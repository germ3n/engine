mod mysql;
mod sqlite;
mod worker;

pub use mysql::Options as MysqlOptions;
pub use sqlite::Sqlite;
pub use worker::{connect, Event, Handle, Inbox, Job};

pub enum Arg {
    Null,
    Int(i64),
    Real(f64),
    Text(String),
    Bytes(Vec<u8>),
}

pub enum Cell {
    Null,
    Int(i64),
    Real(f64),
    Bytes(Vec<u8>),
}

pub struct Outcome {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<Cell>>,
    pub affected: u64,
    pub insert_id: u64,
}

pub struct Statement {
    pub sql: String,
    pub args: Vec<Arg>,
}
