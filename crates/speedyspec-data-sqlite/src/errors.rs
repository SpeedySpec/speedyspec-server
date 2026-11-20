//! SQLite's failures as the data layer's errors.

use rusqlite::ffi;
use speedyspec_data::DbError;

/// SQLite's name for a primary result code.
fn code_name(code: i32) -> &'static str {
    match code & 0xff {
        ffi::SQLITE_ERROR => "SQLITE_ERROR",
        ffi::SQLITE_INTERNAL => "SQLITE_INTERNAL",
        ffi::SQLITE_PERM => "SQLITE_PERM",
        ffi::SQLITE_ABORT => "SQLITE_ABORT",
        ffi::SQLITE_BUSY => "SQLITE_BUSY",
        ffi::SQLITE_LOCKED => "SQLITE_LOCKED",
        ffi::SQLITE_NOMEM => "SQLITE_NOMEM",
        ffi::SQLITE_READONLY => "SQLITE_READONLY",
        ffi::SQLITE_INTERRUPT => "SQLITE_INTERRUPT",
        ffi::SQLITE_IOERR => "SQLITE_IOERR",
        ffi::SQLITE_CORRUPT => "SQLITE_CORRUPT",
        ffi::SQLITE_FULL => "SQLITE_FULL",
        ffi::SQLITE_CANTOPEN => "SQLITE_CANTOPEN",
        ffi::SQLITE_PROTOCOL => "SQLITE_PROTOCOL",
        ffi::SQLITE_SCHEMA => "SQLITE_SCHEMA",
        ffi::SQLITE_TOOBIG => "SQLITE_TOOBIG",
        ffi::SQLITE_CONSTRAINT => "SQLITE_CONSTRAINT",
        ffi::SQLITE_MISMATCH => "SQLITE_MISMATCH",
        ffi::SQLITE_MISUSE => "SQLITE_MISUSE",
        ffi::SQLITE_RANGE => "SQLITE_RANGE",
        ffi::SQLITE_NOTADB => "SQLITE_NOTADB",
        _ => "SQLITE_UNKNOWN",
    }
}

/// `error` from the engine named `engine`.
pub fn map_error(engine: &str, error: rusqlite::Error) -> DbError {
    let engine = engine.to_string();
    match &error {
        rusqlite::Error::SqliteFailure(failure, message) => {
            let message = message.clone().unwrap_or_else(|| failure.to_string());
            match failure.extended_code & 0xff {
                ffi::SQLITE_CONSTRAINT => DbError::Constraint { engine, message },
                ffi::SQLITE_CANTOPEN | ffi::SQLITE_NOTADB => {
                    DbError::Connection { engine, message }
                }
                code => DbError::Engine {
                    engine,
                    code: Some(code_name(code).to_string()),
                    message,
                },
            }
        }
        _ => DbError::Engine {
            engine,
            code: None,
            message: error.to_string(),
        },
    }
}
