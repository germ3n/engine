use super::{Arg, Cell, Outcome};
use ::mysql::consts::ColumnType;
use ::mysql::prelude::Queryable;
use std::time::Duration;

const UNSUPPORTED_IN_PREPARED: u16 = 1295;

pub struct Options {
    pub host: String,
    pub port: u16,
    pub user: Option<String>,
    pub password: Option<String>,
    pub database: Option<String>,
    pub ssl: bool,
    pub ssl_verify: bool,
    pub connect_timeout: Duration,
    pub io_timeout: Duration,
}

pub struct Mysql(::mysql::Conn);

fn mysql_value(arg: Arg) -> ::mysql::Value {
    match arg {
        Arg::Null => ::mysql::Value::NULL,
        Arg::Int(num) => ::mysql::Value::Int(num),
        Arg::Real(num) => ::mysql::Value::Double(num),
        Arg::Text(text) => ::mysql::Value::Bytes(text.into_bytes()),
        Arg::Bytes(bytes) => ::mysql::Value::Bytes(bytes),
    }
}

fn cell(value: ::mysql::Value, kind: ColumnType, text_protocol: bool) -> Cell {
    use ::mysql::Value as V;

    match value {
        V::NULL => Cell::Null,
        V::Int(num) => Cell::Int(num),
        V::UInt(num) => match i64::try_from(num) {
            Ok(num) => Cell::Int(num),
            Err(_) => Cell::Real(num as f64),
        },
        V::Float(num) => Cell::Real(num as f64),
        V::Double(num) => Cell::Real(num),
        V::Date(year, month, day, hour, minute, second, micro) => {
            let date_only = hour == 0 && minute == 0 && second == 0 && micro == 0;
            let text = if kind == ColumnType::MYSQL_TYPE_DATE && date_only {
                format!("{year:04}-{month:02}-{day:02}")
            } else if micro == 0 {
                format!("{year:04}-{month:02}-{day:02} {hour:02}:{minute:02}:{second:02}")
            } else {
                format!(
                    "{year:04}-{month:02}-{day:02} {hour:02}:{minute:02}:{second:02}.{micro:06}"
                )
            };
            Cell::Bytes(text.into_bytes())
        }
        V::Time(negative, days, hours, minutes, seconds, micro) => {
            let total = days * 24 + hours as u32;
            let sign = if negative { "-" } else { "" };
            let text = if micro == 0 {
                format!("{sign}{total:02}:{minutes:02}:{seconds:02}")
            } else {
                format!("{sign}{total:02}:{minutes:02}:{seconds:02}.{micro:06}")
            };
            Cell::Bytes(text.into_bytes())
        }
        V::Bytes(bytes) => {
            if text_protocol {
                let text = std::str::from_utf8(&bytes).unwrap_or("");
                match kind {
                    ColumnType::MYSQL_TYPE_TINY
                    | ColumnType::MYSQL_TYPE_SHORT
                    | ColumnType::MYSQL_TYPE_LONG
                    | ColumnType::MYSQL_TYPE_LONGLONG
                    | ColumnType::MYSQL_TYPE_INT24
                    | ColumnType::MYSQL_TYPE_YEAR => {
                        if let Ok(num) = text.parse::<i64>() {
                            return Cell::Int(num);
                        }
                    }
                    ColumnType::MYSQL_TYPE_FLOAT | ColumnType::MYSQL_TYPE_DOUBLE => {
                        if let Ok(num) = text.parse::<f64>() {
                            return Cell::Real(num);
                        }
                    }
                    _ => {}
                }
            }

            Cell::Bytes(bytes)
        }
    }
}

fn collect<T: ::mysql::prelude::Protocol>(
    mut result: ::mysql::QueryResult<'_, '_, '_, T>,
    want_rows: bool,
    text_protocol: bool,
) -> Result<Outcome, String> {
    let affected = result.affected_rows();
    let insert_id = result.last_insert_id().unwrap_or(0);
    let columns: Vec<(String, ColumnType)> = result
        .columns()
        .as_ref()
        .iter()
        .map(|col| (col.name_str().into_owned(), col.column_type()))
        .collect();
    let mut rows = Vec::new();

    for row in result.by_ref() {
        let row = row.map_err(|err| err.to_string())?;
        if !want_rows {
            continue;
        }

        rows.push(
            row.unwrap()
                .into_iter()
                .enumerate()
                .map(|(idx, value)| {
                    let kind = columns
                        .get(idx)
                        .map(|col| col.1)
                        .unwrap_or(ColumnType::MYSQL_TYPE_VAR_STRING);
                    cell(value, kind, text_protocol)
                })
                .collect(),
        );
    }

    Ok(Outcome {
        columns: columns.into_iter().map(|col| col.0).collect(),
        rows,
        affected,
        insert_id,
    })
}

impl Mysql {
    pub fn connect(options: &Options) -> Result<Self, String> {
        let mut builder = ::mysql::OptsBuilder::new()
            .ip_or_hostname(Some(options.host.clone()))
            .tcp_port(options.port)
            .user(options.user.clone())
            .pass(options.password.clone())
            .db_name(options.database.clone())
            .tcp_connect_timeout(Some(options.connect_timeout))
            .read_timeout(Some(options.io_timeout))
            .write_timeout(Some(options.io_timeout));
        if options.ssl {
            let ssl = ::mysql::SslOpts::default()
                .with_danger_accept_invalid_certs(!options.ssl_verify)
                .with_danger_skip_domain_validation(!options.ssl_verify);
            builder = builder.ssl_opts(Some(ssl));
        }

        ::mysql::Conn::new(builder)
            .map(Self)
            .map_err(|err| err.to_string())
    }

    pub fn run(&mut self, sql: &str, args: Vec<Arg>, want_rows: bool) -> Result<Outcome, String> {
        let conn = &mut self.0;
        let params = if args.is_empty() {
            ::mysql::Params::Empty
        } else {
            ::mysql::Params::Positional(args.into_iter().map(mysql_value).collect())
        };
        let can_fall_back = params == ::mysql::Params::Empty;

        let first = match conn.exec_iter(sql, params) {
            Ok(result) => Ok(collect(result, want_rows, false)),
            Err(err) => Err(err),
        };

        match first {
            Ok(outcome) => outcome,
            Err(::mysql::Error::MySqlError(err))
                if can_fall_back && err.code == UNSUPPORTED_IN_PREPARED =>
            {
                let result = conn.query_iter(sql).map_err(|err| err.to_string())?;
                collect(result, want_rows, true)
            }
            Err(err) => Err(err.to_string()),
        }
    }
}
