//! PostgreSQL's failures as the data layer's errors.

use speedyspec_data::DbError;

/// `e` from the engine named `engine`, by SQLSTATE.
pub fn map_error(engine: &str, e: postgres::Error) -> DbError {
    let engine = engine.to_string();
    if let Some(db) = e.as_db_error() {
        let code = db.code().code().to_string();
        let message = duplicate_entry(db).unwrap_or_else(|| db.message().to_string());
        return match code.as_str() {
            c if c.starts_with("23") => DbError::Constraint { engine, message },
            "40P01" | "40001" => DbError::Deadlock { engine, message },
            // A lost connection, or a server shutting it (57P01 terminated by an administrator).
            c if c.starts_with("08") || c.starts_with("57P0") => {
                DbError::Connection { engine, message }
            }
            _ => DbError::Engine {
                engine,
                code: Some(code),
                message,
            },
        };
    }
    if e.is_closed() || std::error::Error::source(&e).is_some_and(|s| s.is::<std::io::Error>()) {
        return DbError::Connection {
            engine,
            message: e.to_string(),
        };
    }
    DbError::Engine {
        engine,
        code: None,
        message: e.to_string(),
    }
}

/// MySQL's `Duplicate entry '<value>' for key '<table>.<key>'`, from PostgreSQL's unique-violation detail
/// (`Key (id)=(1) already exists.`).
fn duplicate_entry(db: &postgres::error::DbError) -> Option<String> {
    if db.code().code() != "23505" {
        return None;
    }
    let detail = db.detail()?;
    let values = detail
        .split_once(")=(")?
        .1
        .rsplit_once(") already exists")?
        .0;
    let table = db.table()?;
    let constraint = db.constraint()?;
    let key = if constraint.ends_with("_pkey") {
        "PRIMARY".to_string()
    } else {
        constraint
            .split_once("__")
            .map_or(constraint.to_string(), |(_, k)| k.to_string())
    };
    Some(format!(
        "Duplicate entry '{}' for key '{table}.{key}'",
        values.replace(", ", "-")
    ))
}
