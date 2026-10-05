use super::mysql::{Mysql, Options};
use super::{Outcome, Statement};
use std::sync::mpsc::{channel, Receiver, Sender};

pub enum Job {
    Run {
        id: u64,
        statement: Statement,
        want_rows: bool,
    },
    Transaction {
        id: u64,
        statements: Vec<Statement>,
    },
}

pub enum Event {
    Connected {
        id: u64,
        result: Result<Handle, String>,
    },
    Done {
        id: u64,
        result: Result<Vec<Outcome>, String>,
    },
}

pub struct Handle {
    jobs: Sender<Job>,
}

impl Handle {
    pub fn submit(&self, job: Job) -> bool {
        self.jobs.send(job).is_ok()
    }
}

pub struct Inbox {
    tx: Sender<Event>,
    rx: Receiver<Event>,
}

impl Inbox {
    pub fn new() -> Self {
        let (tx, rx) = channel();
        Self { tx, rx }
    }

    pub fn sender(&self) -> Sender<Event> {
        self.tx.clone()
    }

    pub fn drain(&self) -> Vec<Event> {
        self.rx.try_iter().collect()
    }
}

fn transaction(conn: &mut Mysql, statements: Vec<Statement>) -> Result<Vec<Outcome>, String> {
    conn.run("START TRANSACTION", Vec::new(), false)?;

    let mut outcomes = Vec::with_capacity(statements.len());
    for statement in statements {
        match conn.run(&statement.sql, statement.args, false) {
            Ok(outcome) => outcomes.push(outcome),
            Err(err) => {
                let _ = conn.run("ROLLBACK", Vec::new(), false);
                return Err(err);
            }
        }
    }

    if let Err(err) = conn.run("COMMIT", Vec::new(), false) {
        let _ = conn.run("ROLLBACK", Vec::new(), false);
        return Err(err);
    }

    Ok(outcomes)
}

fn serve(mut conn: Mysql, jobs: Receiver<Job>, events: Sender<Event>) {
    for job in jobs {
        let (id, result) = match job {
            Job::Run {
                id,
                statement,
                want_rows,
            } => (
                id,
                conn.run(&statement.sql, statement.args, want_rows)
                    .map(|outcome| vec![outcome]),
            ),
            Job::Transaction { id, statements } => (id, transaction(&mut conn, statements)),
        };

        if events.send(Event::Done { id, result }).is_err() {
            return;
        }
    }
}

pub fn connect(options: Options, id: u64, events: Sender<Event>) {
    let spawned = std::thread::Builder::new()
        .name("mysql".to_string())
        .spawn({
            let events = events.clone();
            move || match Mysql::connect(&options) {
                Ok(conn) => {
                    let (jobs, queue) = channel();
                    let reply = Event::Connected {
                        id,
                        result: Ok(Handle { jobs }),
                    };
                    if events.send(reply).is_ok() {
                        serve(conn, queue, events);
                    }
                }
                Err(err) => {
                    let _ = events.send(Event::Connected {
                        id,
                        result: Err(err),
                    });
                }
            }
        });

    if let Err(err) = spawned {
        let _ = events.send(Event::Connected {
            id,
            result: Err(format!("cannot start worker: {err}")),
        });
    }
}
