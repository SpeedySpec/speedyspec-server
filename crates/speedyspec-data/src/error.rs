//! Errors every store reports.

use std::fmt;

/// The SQL dialects an engine speaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DialectKind {
    MySql,
    Postgres,
    Sqlite,
}

impl fmt::Display for DialectKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            DialectKind::MySql => "mysql",
            DialectKind::Postgres => "postgres",
            DialectKind::Sqlite => "sqlite",
        })
    }
}

/// Every data-access failure.
#[derive(Debug, Clone, PartialEq)]
pub enum DbError {
    Connection { engine: String, message: String },
    Parse { message: String, line: u64, column: u64 },
    Unsupported { construct: String, dialect: DialectKind },
    Constraint { engine: String, message: String },
    Deadlock { engine: String, message: String },
    Engine { engine: String, code: Option<String>, message: String },
    Transaction(String),
    Config(String),
    WrongKind { engine: String, expected: &'static str },
    NoEngine(String),
}

impl fmt::Display for DbError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DbError::Connection { engine, message } => {
                write!(f, "connection to engine `{engine}` failed: {message}")
            }
            DbError::Parse { message, line, column } => {
                write!(f, "SQL parse error at line {line}, column {column}: {message}")
            }
            DbError::Unsupported { construct, dialect } => {
                write!(f, "{construct} is not supported by the {dialect} dialect")
            }
            DbError::Constraint { engine, message } => {
                write!(f, "constraint violated on engine `{engine}`: {message}")
            }
            DbError::Deadlock { engine, message } => {
                write!(f, "deadlock on engine `{engine}`: {message}")
            }
            DbError::Engine { engine, code, message } => match code {
                Some(code) => write!(f, "engine `{engine}` error {code}: {message}"),
                None => write!(f, "engine `{engine}` error: {message}"),
            },
            DbError::Transaction(message) => write!(f, "transaction error: {message}"),
            DbError::Config(message) => write!(f, "configuration error: {message}"),
            DbError::WrongKind { engine, expected } => {
                write!(f, "engine `{engine}` is not a {expected} store")
            }
            DbError::NoEngine(name) => write!(f, "no engine named `{name}` is configured"),
        }
    }
}

impl std::error::Error for DbError {}
