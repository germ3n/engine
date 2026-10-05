use super::{Arg, Cell, Outcome};
use std::time::Duration;

const BUSY_MS: u64 = 5000;

pub struct Sqlite(rusqlite::Connection);

fn sqlite_value(arg: Arg) -> rusqlite::types::Value {
    use rusqlite::types::Value as V;
    match arg {
        Arg::Null => V::Null,
        Arg::Int(num) => V::Integer(num),
        Arg::Real(num) => V::Real(num),
        Arg::Text(text) => V::Text(text),
        Arg::Bytes(bytes) => V::Blob(bytes),
    }
}

impl Sqlite {
    pub fn open(path: &str) -> Result<Self, String> {
        let path = path.trim();
        let conn = if path == ":memory:" {
            rusqlite::Connection::open_in_memory()
        } else {
            rusqlite::Connection::open(crate::world::expand_home(path))
        }
        .map_err(|err| err.to_string())?;
        let _ = conn.busy_timeout(Duration::from_millis(BUSY_MS));

        Ok(Self(conn))
    }

    pub fn run(&self, sql: &str, args: Vec<Arg>, want_rows: bool) -> Result<Outcome, String> {
        use rusqlite::types::Value as V;

        let conn = &self.0;
        let mut outcome = Outcome {
            columns: Vec::new(),
            rows: Vec::new(),
            affected: 0,
            insert_id: 0,
        };

        if !want_rows && args.is_empty() {
            conn.execute_batch(sql).map_err(|err| err.to_string())?;
        } else {
            let mut stmt = conn.prepare(sql).map_err(|err| err.to_string())?;
            let params = rusqlite::params_from_iter(args.into_iter().map(sqlite_value));
            let count = stmt.column_count();
            outcome.columns = stmt
                .column_names()
                .iter()
                .map(|name| name.to_string())
                .collect();
            let mut rows = stmt.query(params).map_err(|err| err.to_string())?;
            while let Some(row) = rows.next().map_err(|err| err.to_string())? {
                let mut cells = Vec::with_capacity(count);
                for idx in 0..count {
                    let value: V = row.get(idx).map_err(|err| err.to_string())?;
                    cells.push(match value {
                        V::Null => Cell::Null,
                        V::Integer(num) => Cell::Int(num),
                        V::Real(num) => Cell::Real(num),
                        V::Text(text) => Cell::Bytes(text.into_bytes()),
                        V::Blob(bytes) => Cell::Bytes(bytes),
                    });
                }
                if want_rows {
                    outcome.rows.push(cells);
                }
            }
        }

        outcome.affected = conn.changes();
        outcome.insert_id = conn.last_insert_rowid().max(0) as u64;
        Ok(outcome)
    }
}
