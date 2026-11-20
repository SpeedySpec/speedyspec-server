//! Opening a SQLite connection the way the engine uses it.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rusqlite::Connection;
use speedyspec_data::DbError;
use speedyspec_data_translate::SessionVars;

/// Where the database lives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SqliteTarget {
    Memory,
    File(PathBuf),
}

fn engine_error(e: rusqlite::Error) -> DbError {
    DbError::Connection {
        engine: "sqlite".into(),
        message: e.to_string(),
    }
}

/// A connection with WAL for files, the busy timeout, no foreign keys (MySQL's MyISAM-era WordPress tables declare
/// none), an attached `information_schema`, and MySQL's collation and functions.
pub fn open_connection(
    target: &SqliteTarget,
    busy_timeout: Duration,
    session: Arc<Mutex<SessionVars>>,
) -> Result<Connection, DbError> {
    let conn = match target {
        SqliteTarget::Memory => Connection::open_in_memory(),
        SqliteTarget::File(path) => Connection::open(path),
    }
    .map_err(engine_error)?;
    if matches!(target, SqliteTarget::File(_)) {
        conn.pragma_update(None, "journal_mode", "WAL")
            .map_err(engine_error)?;
        conn.pragma_update(None, "synchronous", "NORMAL")
            .map_err(engine_error)?;
    }
    conn.busy_timeout(busy_timeout).map_err(engine_error)?;
    // Every name is double-quoted; SQLite must not read a quoted name that matches nothing as a string.
    for flag in [
        rusqlite::config::DbConfig::SQLITE_DBCONFIG_DQS_DML,
        rusqlite::config::DbConfig::SQLITE_DBCONFIG_DQS_DDL,
    ] {
        conn.set_db_config(flag, false).map_err(engine_error)?;
    }
    conn.pragma_update(None, "foreign_keys", "OFF")
        .map_err(engine_error)?;
    conn.execute_batch("ATTACH DATABASE ':memory:' AS information_schema")
        .map_err(engine_error)?;
    crate::register(&conn, session).map_err(engine_error)?;
    Ok(conn)
}
