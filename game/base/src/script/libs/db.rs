use crate::db::{self, Arg, Cell, Handle, Inbox, Job, MysqlOptions, Outcome, Sqlite, Statement};
use mlua::prelude::LuaUserDataMethods;
use mlua::{Function, Lua, MultiValue, Table, UserData, Value, Variadic};
use r#macro::document;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

const MYSQL_CONNECT_SECS: u64 = 10;
const MYSQL_IO_SECS: f64 = 30.0;

enum Kind {
    Connect,
    Query,
    Exec,
    Transaction,
}

struct Pending {
    kind: Kind,
    callback: Option<Function>,
}

struct DbState {
    inbox: Inbox,
    next_id: u64,
    pending: HashMap<u64, Pending>,
}

fn arg_from_lua(value: &Value) -> Result<Arg, String> {
    Ok(match value {
        Value::Nil => Arg::Null,
        Value::Boolean(flag) => Arg::Int(*flag as i64),
        Value::Integer(num) => Arg::Int(*num as i64),
        Value::Number(num) => Arg::Real(*num),
        Value::String(text) => match String::from_utf8(text.as_bytes().to_vec()) {
            Ok(text) => Arg::Text(text),
            Err(err) => Arg::Bytes(err.into_bytes()),
        },
        other => return Err(format!("cannot bind a {} value", other.type_name())),
    })
}

fn lua_args(args: &[Value]) -> Result<Vec<Arg>, String> {
    args.iter().map(arg_from_lua).collect()
}

fn cell_to_lua(lua: &Lua, cell: Cell) -> mlua::Result<Value> {
    Ok(match cell {
        Cell::Null => Value::Nil,
        Cell::Int(num) => Value::Integer(num as _),
        Cell::Real(num) => Value::Number(num),
        Cell::Bytes(bytes) => Value::String(lua.create_string(&bytes)?),
    })
}

fn rows_to_lua(lua: &Lua, outcome: Outcome) -> mlua::Result<Table> {
    let rows = lua.create_table_with_capacity(outcome.rows.len(), 0)?;
    for cells in outcome.rows {
        let row = lua.create_table_with_capacity(0, cells.len())?;
        for (name, cell) in outcome.columns.iter().zip(cells) {
            row.set(name.as_str(), cell_to_lua(lua, cell)?)?;
        }
        rows.push(row)?;
    }

    Ok(rows)
}

fn fail(lua: &Lua, message: &str) -> mlua::Result<MultiValue> {
    Ok(MultiValue::from_iter([
        Value::Nil,
        Value::String(lua.create_string(message)?),
    ]))
}

fn split_callback(mut args: Vec<Value>) -> (Vec<Value>, Option<Function>) {
    if matches!(args.last(), Some(Value::Function(_))) {
        if let Some(Value::Function(callback)) = args.pop() {
            return (args, Some(callback));
        }
    }

    (args, None)
}

struct LuaSqlite {
    db: Mutex<Option<Sqlite>>,
}

impl LuaSqlite {
    fn run(&self, sql: &str, args: Vec<Arg>, want_rows: bool) -> Result<Outcome, String> {
        let slot = self
            .db
            .try_lock()
            .map_err(|_| "database is busy".to_string())?;
        let db = slot.as_ref().ok_or_else(|| "database is closed".to_string())?;

        db.run(sql, args, want_rows)
    }

    fn is_open(&self) -> bool {
        self.db.try_lock().map(|slot| slot.is_some()).unwrap_or(true)
    }
}

impl UserData for LuaSqlite {
    fn add_methods<M: LuaUserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("query", |lua, this, (sql, args): (String, Variadic<Value>)| {
            let args = match lua_args(&args) {
                Ok(args) => args,
                Err(err) => return fail(lua, &err),
            };
            match this.run(&sql, args, true) {
                Ok(outcome) => Ok(MultiValue::from_iter([Value::Table(rows_to_lua(
                    lua, outcome,
                )?)])),
                Err(err) => fail(lua, &err),
            }
        });

        methods.add_method("exec", |lua, this, (sql, args): (String, Variadic<Value>)| {
            let args = match lua_args(&args) {
                Ok(args) => args,
                Err(err) => return fail(lua, &err),
            };
            match this.run(&sql, args, false) {
                Ok(outcome) => Ok(MultiValue::from_iter([
                    Value::Number(outcome.affected as f64),
                    Value::Number(outcome.insert_id as f64),
                ])),
                Err(err) => fail(lua, &err),
            }
        });

        methods.add_method("transaction", |lua, this, body: Function| {
            if let Err(err) = this.run("BEGIN", Vec::new(), false) {
                return fail(lua, &err);
            }

            match body.call::<MultiValue>(()) {
                Ok(values) => {
                    if let Err(err) = this.run("COMMIT", Vec::new(), false) {
                        let _ = this.run("ROLLBACK", Vec::new(), false);
                        return fail(lua, &err);
                    }

                    let mut out = MultiValue::from_iter([Value::Boolean(true)]);
                    out.extend(values);
                    Ok(out)
                }
                Err(err) => {
                    let _ = this.run("ROLLBACK", Vec::new(), false);
                    fail(lua, &err.to_string())
                }
            }
        });

        methods.add_method("close", |_, this, ()| {
            Ok(this.db.try_lock().map(|mut slot| slot.take()).is_ok())
        });

        methods.add_method("is_open", |_, this, ()| Ok(this.is_open()));

        methods.add_meta_method("__tostring", |_, this, ()| {
            Ok(format!(
                "SqliteDatabase({})",
                if this.is_open() { "open" } else { "closed" }
            ))
        });
    }
}

struct LuaMysql {
    handle: Mutex<Option<Handle>>,
}

impl LuaMysql {
    fn is_open(&self) -> bool {
        self.handle
            .lock()
            .map(|slot| slot.is_some())
            .unwrap_or(false)
    }

    fn submit(
        &self,
        lua: &Lua,
        kind: Kind,
        callback: Option<Function>,
        make: impl FnOnce(u64) -> Job,
    ) -> Result<(), String> {
        let slot = self
            .handle
            .lock()
            .map_err(|_| "database is busy".to_string())?;
        let handle = slot
            .as_ref()
            .ok_or_else(|| "database is closed".to_string())?;
        let mut state = lua
            .app_data_mut::<DbState>()
            .ok_or_else(|| "mysql is not available".to_string())?;

        let id = state.next_id;
        state.next_id += 1;
        if !handle.submit(make(id)) {
            return Err("connection lost".to_string());
        }

        state.pending.insert(id, Pending { kind, callback });
        Ok(())
    }
}

fn statement(sql: String, args: Vec<Value>) -> Result<Statement, String> {
    Ok(Statement {
        sql,
        args: lua_args(&args)?,
    })
}

fn queued(lua: &Lua, result: Result<(), String>) -> mlua::Result<MultiValue> {
    match result {
        Ok(()) => Ok(MultiValue::from_iter([Value::Boolean(true)])),
        Err(err) => fail(lua, &err),
    }
}

fn parse_statements(list: Table) -> Result<Vec<Statement>, String> {
    let mut statements = Vec::new();
    for entry in list.sequence_values::<Table>() {
        let entry = entry.map_err(|_| "each statement must be a table".to_string())?;
        let sql: String = entry
            .get(1)
            .map_err(|_| "each statement starts with its sql string".to_string())?;
        let mut args = Vec::new();
        for idx in 2..=entry.raw_len() {
            args.push(entry.raw_get::<Value>(idx).map_err(|err| err.to_string())?);
        }
        statements.push(statement(sql, args)?);
    }

    Ok(statements)
}

impl UserData for LuaMysql {
    fn add_methods<M: LuaUserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("query", |lua, this, (sql, args): (String, Variadic<Value>)| {
            let (args, callback) = split_callback(args.into_iter().collect());
            let statement = match statement(sql, args) {
                Ok(statement) => statement,
                Err(err) => return fail(lua, &err),
            };
            let result = this.submit(lua, Kind::Query, callback, |id| Job::Run {
                id,
                statement,
                want_rows: true,
            });
            queued(lua, result)
        });

        methods.add_method("exec", |lua, this, (sql, args): (String, Variadic<Value>)| {
            let (args, callback) = split_callback(args.into_iter().collect());
            let statement = match statement(sql, args) {
                Ok(statement) => statement,
                Err(err) => return fail(lua, &err),
            };
            let result = this.submit(lua, Kind::Exec, callback, |id| Job::Run {
                id,
                statement,
                want_rows: false,
            });
            queued(lua, result)
        });

        methods.add_method(
            "transaction",
            |lua, this, (list, callback): (Table, Option<Function>)| {
                let statements = match parse_statements(list) {
                    Ok(statements) => statements,
                    Err(err) => return fail(lua, &err),
                };
                let result = this.submit(lua, Kind::Transaction, callback, |id| {
                    Job::Transaction { id, statements }
                });
                queued(lua, result)
            },
        );

        methods.add_method("close", |_, this, ()| {
            Ok(this
                .handle
                .lock()
                .map(|mut slot| slot.take().is_some())
                .unwrap_or(false))
        });

        methods.add_method("is_open", |_, this, ()| Ok(this.is_open()));

        methods.add_meta_method("__tostring", |_, this, ()| {
            Ok(format!(
                "MysqlDatabase({})",
                if this.is_open() { "open" } else { "closed" }
            ))
        });
    }
}

fn deliver(lua: &Lua, pending: Pending, event: db::Event) -> mlua::Result<()> {
    let Pending { kind, callback } = pending;

    let args: Result<MultiValue, String> = match (kind, event) {
        (Kind::Connect, db::Event::Connected { result, .. }) => result.and_then(|handle| {
            let db = LuaMysql {
                handle: Mutex::new(Some(handle)),
            };
            lua.create_userdata(db)
                .map(|db| MultiValue::from_iter([Value::UserData(db)]))
                .map_err(|err| err.to_string())
        }),
        (kind, db::Event::Done { result, .. }) => result.and_then(|outcomes| {
            let first = outcomes.into_iter().next();
            let values = |first: Option<Outcome>| -> mlua::Result<MultiValue> {
                Ok(match (kind, first) {
                    (Kind::Query, Some(outcome)) => {
                        MultiValue::from_iter([Value::Table(rows_to_lua(lua, outcome)?)])
                    }
                    (Kind::Exec, Some(outcome)) => MultiValue::from_iter([
                        Value::Number(outcome.affected as f64),
                        Value::Number(outcome.insert_id as f64),
                    ]),
                    _ => MultiValue::from_iter([Value::Boolean(true)]),
                })
            };
            values(first).map_err(|err| err.to_string())
        }),
        _ => Err("unexpected database event".to_string()),
    };

    match (callback, args) {
        (Some(callback), Ok(args)) => callback.call::<()>(args),
        (Some(callback), Err(err)) => callback.call::<()>((Value::Nil, err)),
        (None, Err(err)) => {
            log::warn!("[mysql] {err}");
            Ok(())
        }
        (None, Ok(_)) => Ok(()),
    }
}

pub fn pump(lua: &Lua) {
    let ready: Vec<(Pending, db::Event)> = {
        let Some(mut state) = lua.app_data_mut::<DbState>() else {
            return;
        };

        let events = state.inbox.drain();
        events
            .into_iter()
            .filter_map(|event| {
                let id = match &event {
                    db::Event::Connected { id, .. } | db::Event::Done { id, .. } => *id,
                };
                state.pending.remove(&id).map(|pending| (pending, event))
            })
            .collect()
    };

    for (pending, event) in ready {
        if let Err(err) = deliver(lua, pending, event) {
            log::error!("[LUA DB ERROR]: {err}");
        }
    }
}

#[document(
    kind = "library",
    name = "sqlite",
    realm = "shared",
    summary = "Embedded SQLite databases. Calls run immediately and block the game thread until they finish, so keep queries small. Scripts already have unrestricted io and os, so a database path is not sandboxed."
)]
fn sqlite_lib() {}

#[document(
    parent = "sqlite",
    name = "open",
    kind = "function",
    realm = "shared",
    summary = "Opens a database file, creating it when missing. Waits up to five seconds on a locked database.",
    params = {
        path = { ty = "string", desc = "File path, or :memory: for a private in-memory database. A leading ~ is the home folder." },
    },
    returns = { ty = "SqliteDatabase", desc = "The database. On failure nil, then a message." },
    example = "local db = assert(sqlite.open(\"~/scores.db\"))\ndb:exec(\"CREATE TABLE IF NOT EXISTS scores (name TEXT, score INTEGER)\")",
)]
fn sqlite_open() {}

#[document(
    kind = "class",
    name = "SqliteDatabase",
    realm = "shared",
    summary = "An open sqlite database. Statements use ? placeholders bound from the extra arguments, never build SQL by concatenating user input. Numbers are Lua doubles, so integers beyond 2^53 lose precision. Text and blobs are strings and NULL is nil, so a NULL column is missing from its row table."
)]
fn database_class() {}

#[document(
    parent = "SqliteDatabase",
    name = "query",
    kind = "method",
    realm = "shared",
    summary = "Runs a statement and returns its rows.",
    params = {
        sql = { ty = "string", desc = "The statement, with ? placeholders." },
        args = { ty = "any", desc = "One value per placeholder. nil, booleans, numbers and strings can be bound.", optional = true },
    },
    returns = { ty = "table", desc = "A list of rows, each keyed by column name. On failure nil, then a message." },
    example = "for _, row in ipairs(db:query(\"SELECT name FROM scores WHERE score > ?\", 10)) do print(row.name) end",
)]
fn database_query() {}

#[document(
    parent = "SqliteDatabase",
    name = "exec",
    kind = "method",
    realm = "shared",
    summary = "Runs a statement and discards any rows. Without arguments sqlite accepts several statements separated by semicolons.",
    params = {
        sql = { ty = "string", desc = "The statement, with ? placeholders." },
        args = { ty = "any", desc = "One value per placeholder.", optional = true },
    },
    returns = { ty = "number", desc = "Rows changed, then the last insert id. On failure nil, then a message." },
)]
fn database_exec() {}

#[document(
    parent = "SqliteDatabase",
    name = "transaction",
    kind = "method",
    realm = "shared",
    summary = "Runs a function inside a transaction. It commits when the function returns and rolls back when it errors. Do not nest transactions.",
    params = {
        body = { ty = "function", desc = "Called with no arguments. Use this database inside it." },
    },
    returns = { ty = "boolean", desc = "True followed by whatever the function returned. On failure nil, then the error." },
    example = "db:transaction(function()\n    db:exec(\"UPDATE bank SET coins = coins - 5 WHERE id = ?\", a)\n    db:exec(\"UPDATE bank SET coins = coins + 5 WHERE id = ?\", b)\nend)",
)]
fn database_transaction() {}

#[document(
    parent = "SqliteDatabase",
    name = "close",
    kind = "method",
    realm = "shared",
    summary = "Closes the database. It also closes when garbage collected.",
    returns = { ty = "boolean", desc = "True when closed." },
)]
fn database_close() {}

#[document(
    parent = "SqliteDatabase",
    name = "is_open",
    kind = "method",
    realm = "shared",
    summary = "Whether the database is still open.",
    returns = { ty = "boolean", desc = "False after close." },
)]
fn database_is_open() {}

#[document(
    kind = "library",
    name = "mysql",
    realm = "server",
    summary = "MySQL and MariaDB client, server only. Connections run on their own thread, so nothing here blocks the game. Every call returns at once and its result arrives in a callback during a later tick. Jobs on one connection run in the order they were queued. Scripts already have unrestricted io and os, so connections are not sandboxed."
)]
fn mysql_lib() {}

#[document(
    parent = "mysql",
    name = "connect",
    kind = "function",
    realm = "server",
    summary = "Connects in the background. The connection is encrypted and the server certificate verified by default. Set ssl_verify = false to accept self-signed or otherwise invalid certificates: the traffic stays encrypted but the server is not authenticated, so an active attacker can impersonate it. Set ssl = false to turn encryption off, which sends the password handshake and all data in the clear.",
    params = {
        options = { ty = "table", desc = "host (default 127.0.0.1), port (default 3306), user, password, database, ssl (default true), ssl_verify (default true, false accepts self-signed certificates), timeout (seconds for each read and write, default 30)." },
        callback = { ty = "function", desc = "Called with the MysqlDatabase, or nil and a message when the connection failed." },
    },
    returns = { ty = "boolean", desc = "True once queued. On bad options nil, then a message." },
    example = "mysql.connect({ host = \"db.example.com\", user = \"game\", password = pw, database = \"stats\" }, function(db, err)\n    if not db then return print(\"mysql: \" .. err) end\n    db:query(\"SELECT name FROM scores WHERE score > ?\", 10, function(rows, err)\n        for _, row in ipairs(rows) do print(row.name) end\n    end)\nend)",
)]
fn mysql_connect() {}

#[document(
    kind = "class",
    name = "MysqlDatabase",
    realm = "server",
    summary = "An open mysql connection. Statements use ? placeholders bound from the arguments before the optional trailing callback, never build SQL by concatenating user input. Numbers are Lua doubles, so integers beyond 2^53 lose precision. Text and blobs are strings, NULL is nil, and dates and times are strings. Without a callback a failure is only logged."
)]
fn mysql_database_class() {}

#[document(
    parent = "MysqlDatabase",
    name = "query",
    kind = "method",
    realm = "server",
    summary = "Queues a statement and passes its rows to the callback.",
    params = {
        sql = { ty = "string", desc = "The statement, with ? placeholders." },
        args = { ty = "any", desc = "One value per placeholder, followed by the callback. The callback gets the list of rows keyed by column name, or nil and a message.", optional = true },
    },
    returns = { ty = "boolean", desc = "True once queued. On failure nil, then a message such as database is closed." },
)]
fn mysql_database_query() {}

#[document(
    parent = "MysqlDatabase",
    name = "exec",
    kind = "method",
    realm = "server",
    summary = "Queues a statement and discards any rows. The callback gets the rows changed and the last insert id, or nil and a message.",
    params = {
        sql = { ty = "string", desc = "The statement, with ? placeholders." },
        args = { ty = "any", desc = "One value per placeholder, followed by the callback.", optional = true },
    },
    returns = { ty = "boolean", desc = "True once queued. On failure nil, then a message." },
)]
fn mysql_database_exec() {}

#[document(
    parent = "MysqlDatabase",
    name = "transaction",
    kind = "method",
    realm = "server",
    summary = "Runs several statements atomically. Any failure rolls all of them back. The callback gets true, or nil and a message.",
    params = {
        statements = { ty = "table", desc = "A list of tables, each the sql string followed by its arguments." },
        callback = { ty = "function", desc = "Called when the transaction finished.", optional = true },
    },
    returns = { ty = "boolean", desc = "True once queued. On failure nil, then a message." },
    example = "db:transaction({\n    { \"UPDATE bank SET coins = coins - ? WHERE id = ?\", 5, a },\n    { \"UPDATE bank SET coins = coins + ? WHERE id = ?\", 5, b },\n}, function(ok, err) end)",
)]
fn mysql_database_transaction() {}

#[document(
    parent = "MysqlDatabase",
    name = "close",
    kind = "method",
    realm = "server",
    summary = "Stops accepting statements. Statements already queued still run and still call back, then the connection closes.",
    returns = { ty = "boolean", desc = "False when already closed." },
)]
fn mysql_database_close() {}

#[document(
    parent = "MysqlDatabase",
    name = "is_open",
    kind = "method",
    realm = "server",
    summary = "Whether statements can still be queued.",
    returns = { ty = "boolean", desc = "False after close." },
)]
fn mysql_database_is_open() {}

fn mysql_options(options: &Table) -> Result<MysqlOptions, String> {
    let text = |key: &str| -> Result<Option<String>, String> {
        options
            .get::<Option<String>>(key)
            .map_err(|err| format!("{key}: {err}"))
    };
    let io = options
        .get::<Option<f64>>("timeout")
        .map_err(|err| format!("timeout: {err}"))?
        .unwrap_or(MYSQL_IO_SECS);
    if !io.is_finite() || io <= 0.0 {
        return Err("timeout: must be a positive number".to_string());
    }

    Ok(MysqlOptions {
        host: text("host")?.unwrap_or_else(|| "127.0.0.1".to_string()),
        port: options
            .get::<Option<u16>>("port")
            .map_err(|err| format!("port: {err}"))?
            .unwrap_or(3306),
        user: text("user")?,
        password: text("password")?,
        database: text("database")?,
        ssl: options
            .get::<Option<bool>>("ssl")
            .map_err(|err| format!("ssl: {err}"))?
            .unwrap_or(true),
        ssl_verify: options
            .get::<Option<bool>>("ssl_verify")
            .map_err(|err| format!("ssl_verify: {err}"))?
            .unwrap_or(true),
        connect_timeout: Duration::from_secs(MYSQL_CONNECT_SECS),
        io_timeout: Duration::from_secs_f64(io),
    })
}

pub fn register_db_libs(lua: &Lua, server: bool) {
    let sqlite = lua.create_table().expect("Failed to create sqlite table");
    sqlite
        .set(
            "open",
            lua.create_function(|lua, path: String| match Sqlite::open(&path) {
                Ok(db) => {
                    let db = LuaSqlite {
                        db: Mutex::new(Some(db)),
                    };
                    Ok(MultiValue::from_iter([Value::UserData(
                        lua.create_userdata(db)?,
                    )]))
                }
                Err(err) => fail(lua, &err),
            })
            .expect("[sqlite] Failed to create open"),
        )
        .expect("[sqlite] Failed setting open");
    lua.globals()
        .set("sqlite", sqlite)
        .expect("Failed to set sqlite table");

    if !server {
        return;
    }

    lua.set_app_data(DbState {
        inbox: Inbox::new(),
        next_id: 1,
        pending: HashMap::new(),
    });

    let mysql = lua.create_table().expect("Failed to create mysql table");
    mysql
        .set(
            "connect",
            lua.create_function(|lua, (options, callback): (Table, Function)| {
                let options = match mysql_options(&options) {
                    Ok(options) => options,
                    Err(err) => return fail(lua, &err),
                };
                let mut state = lua
                    .app_data_mut::<DbState>()
                    .ok_or_else(|| mlua::Error::external("mysql is not available"))?;

                let id = state.next_id;
                state.next_id += 1;
                state.pending.insert(
                    id,
                    Pending {
                        kind: Kind::Connect,
                        callback: Some(callback),
                    },
                );
                db::connect(options, id, state.inbox.sender());

                Ok(MultiValue::from_iter([Value::Boolean(true)]))
            })
            .expect("[mysql] Failed to create connect"),
        )
        .expect("[mysql] Failed setting connect");
    lua.globals()
        .set("mysql", mysql)
        .expect("Failed to set mysql table");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lua(server: bool) -> Lua {
        let lua = Lua::new();
        register_db_libs(&lua, server);
        lua
    }

    #[test]
    fn sqlite_round_trip() {
        let out: String = lua(false)
            .load(
                r#"
                local db = assert(sqlite.open(":memory:"))
                assert(db:exec("CREATE TABLE t (id INTEGER PRIMARY KEY, name TEXT, score REAL, blob BLOB)"))
                local changed, id = db:exec("INSERT INTO t (name, score, blob) VALUES (?, ?, ?)", "o'brien", 1.5, "\0\1")
                assert(changed == 1 and id == 1)
                db:exec("INSERT INTO t (name) VALUES (?)", nil)
                local rows = assert(db:query("SELECT * FROM t ORDER BY id"))
                assert(#rows == 2)
                assert(rows[1].id == 1)
                assert(rows[1].name == "o'brien" and rows[1].score == 1.5 and #rows[1].blob == 2)
                assert(rows[2].name == nil)
                local ok = db:transaction(function() db:exec("DELETE FROM t") error("nope") end)
                assert(ok == nil)
                assert(#db:query("SELECT * FROM t") == 2)
                local bad, err = db:query("SELEC 1")
                assert(bad == nil and err:find("syntax"))
                db:close()
                local closed, why = db:query("SELECT 1")
                assert(closed == nil and why == "database is closed")
                return tostring(db)
                "#,
            )
            .eval()
            .unwrap();
        assert_eq!(out, "SqliteDatabase(closed)");
    }

    #[test]
    fn mysql_is_server_only() {
        let client = lua(false);
        let missing: bool = client.load("return mysql == nil").eval().unwrap();
        assert!(missing);
        let server = lua(true);
        let present: bool = server.load("return mysql ~= nil").eval().unwrap();
        assert!(present);
    }

    #[test]
    fn mysql_connect_failure_reaches_the_callback() {
        let lua = lua(true);
        lua.load(
            r#"
            result = nil
            assert(mysql.connect({ port = 1, ssl = false, timeout = 1 }, function(db, err)
                result = { db = db, err = err }
            end))
            local bad, why = mysql.connect({ port = "x" }, function() end)
            assert(bad == nil and why:find("port"))
            "#,
        )
        .exec()
        .unwrap();

        for _ in 0..200 {
            pump(&lua);
            let done: bool = lua.load("return result ~= nil").eval().unwrap();
            if done {
                break;
            }
            std::thread::sleep(Duration::from_millis(25));
        }

        let ok: bool = lua
            .load("return result ~= nil and result.db == nil and type(result.err) == 'string'")
            .eval()
            .unwrap();
        assert!(ok, "callback was not called with an error");
    }
}
