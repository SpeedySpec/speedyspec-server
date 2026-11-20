//! MySQL's `information_schema.tables`, `columns` and `statistics`, kept in the attached in-memory database and
//! rewritten from the introspector before a statement reads them.

use rusqlite::{Connection, params};
use speedyspec_data::Introspector;
use speedyspec_data_translate::MySqlType;

/// The three tables, created once per connection.
pub const CREATE: &str = "\
CREATE TABLE IF NOT EXISTS information_schema.tables (TABLE_CATALOG TEXT, TABLE_SCHEMA TEXT, TABLE_NAME TEXT, TABLE_TYPE TEXT, ENGINE TEXT, TABLE_ROWS INTEGER, DATA_LENGTH INTEGER, INDEX_LENGTH INTEGER, AUTO_INCREMENT INTEGER, TABLE_COLLATION TEXT);
CREATE TABLE IF NOT EXISTS information_schema.columns (TABLE_SCHEMA TEXT, TABLE_NAME TEXT, COLUMN_NAME TEXT, ORDINAL_POSITION INTEGER, COLUMN_DEFAULT TEXT, IS_NULLABLE TEXT, DATA_TYPE TEXT, CHARACTER_MAXIMUM_LENGTH INTEGER, COLUMN_TYPE TEXT, COLUMN_KEY TEXT, EXTRA TEXT, COLLATION_NAME TEXT, CHARACTER_SET_NAME TEXT);
CREATE TABLE IF NOT EXISTS information_schema.statistics (TABLE_SCHEMA TEXT, TABLE_NAME TEXT, NON_UNIQUE INTEGER, INDEX_NAME TEXT, SEQ_IN_INDEX INTEGER, COLUMN_NAME TEXT, SUB_PART INTEGER);";

fn max_length(ty: &MySqlType) -> Option<i64> {
    match ty.base.as_str() {
        "char" | "varchar" | "binary" | "varbinary" => ty.args.first().and_then(|a| a.parse().ok()),
        "tinytext" | "tinyblob" => Some(255),
        "text" | "blob" => Some(65535),
        "mediumtext" | "mediumblob" => Some(16_777_215),
        "longtext" | "longblob" => Some(4_294_967_295),
        _ => None,
    }
}

/// Rewrites the three tables for `database` from `introspector`.
pub fn refresh(
    conn: &Connection,
    introspector: &dyn Introspector,
    database: &str,
) -> Result<(), speedyspec_data::DbError> {
    let engine = |e: rusqlite::Error| crate::errors::map_error("sqlite", e);
    conn.execute_batch(
        "DELETE FROM information_schema.tables; DELETE FROM information_schema.columns; DELETE FROM information_schema.statistics;",
    )
    .map_err(engine)?;
    for table in introspector.tables(None)? {
        let rows: i64 = conn
            .query_row(
                &format!("SELECT COUNT(*) FROM \"{}\"", table.replace('"', "\"\"")),
                [],
                |r| r.get(0),
            )
            .map_err(engine)?;
        let columns = introspector.columns(&table)?;
        let auto = columns.iter().any(|c| c.extra == "auto_increment");
        let next: Option<i64> = if auto {
            Some(
                conn.query_row(
                    "SELECT seq FROM main.sqlite_sequence WHERE name = ?1",
                    [&table],
                    |r| r.get::<_, i64>(0),
                )
                .map_or(1, |n| n + 1),
            )
        } else {
            None
        };
        conn.execute(
            "INSERT INTO information_schema.tables VALUES ('def', ?1, ?2, 'BASE TABLE', 'InnoDB', ?3, 0, 0, ?4, ?5)",
            params![database, table, rows, next, introspector.table_collation(&table)?],
        )
        .map_err(engine)?;
        for (i, c) in columns.iter().enumerate() {
            let ty = MySqlType::parse(&c.sql_type)?;
            let charset = c
                .collation
                .as_deref()
                .map(|n| n.split('_').next().unwrap_or(n).to_string());
            conn.execute(
                "INSERT INTO information_schema.columns VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
                params![
                    database,
                    table,
                    c.name,
                    i as i64 + 1,
                    c.default,
                    if c.nullable { "YES" } else { "NO" },
                    ty.base,
                    max_length(&ty),
                    c.sql_type,
                    c.key,
                    c.extra,
                    c.collation,
                    charset
                ],
            )
            .map_err(engine)?;
        }
        for index in introspector.indexes(&table)? {
            for (seq, (column, sub)) in index.columns.iter().enumerate() {
                conn.execute(
                    "INSERT INTO information_schema.statistics VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                    params![
                        database,
                        table,
                        i64::from(!index.unique),
                        index.name,
                        seq as i64 + 1,
                        column,
                        sub
                    ],
                )
                .map_err(engine)?;
            }
        }
    }
    Ok(())
}
