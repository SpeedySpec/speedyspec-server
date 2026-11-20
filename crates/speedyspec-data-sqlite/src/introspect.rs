//! The SQLite schema in MySQL's terms: the catalogue first, SQLite's own description for tables it does not record.

use rusqlite::Connection;
use speedyspec_data::{ColumnDef, DbError, IndexDef};

/// MySQL's name for a type SQLite only knows by affinity.
fn affinity_type(declared: &str) -> &'static str {
    let d = declared.to_ascii_uppercase();
    if d.contains("INT") {
        "bigint"
    } else if d.contains("CHAR") || d.contains("CLOB") || d.contains("TEXT") {
        "longtext"
    } else if d.contains("BLOB") || d.is_empty() {
        "longblob"
    } else {
        "double"
    }
}

/// Tables SQLite has that are neither its own, the catalogue's nor scratch tables.
pub fn engine_tables(conn: &Connection) -> Result<Vec<String>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT name FROM main.sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite\\_%' ESCAPE '\\' \
         AND name NOT LIKE '\\_speedyspec\\_%' ESCAPE '\\' AND name NOT LIKE '%\\_\\_speedyspec\\_rebuild' ESCAPE '\\' \
         ORDER BY name",
    )?;
    let names = stmt.query_map([], |r| r.get::<_, String>(0))?;
    names.collect()
}

/// `PRAGMA table_info` as MySQL columns.
pub fn pragma_columns(conn: &Connection, table: &str) -> Result<Vec<ColumnDef>, rusqlite::Error> {
    let mut stmt =
        conn.prepare("SELECT name, type, \"notnull\", dflt_value, pk FROM pragma_table_info(?1)")?;
    let rows = stmt.query_map([table], |r| {
        let pk: i64 = r.get(4)?;
        Ok(ColumnDef {
            name: r.get(0)?,
            sql_type: affinity_type(&r.get::<_, String>(1)?).to_string(),
            nullable: r.get::<_, i64>(2)? == 0 && pk == 0,
            key: if pk > 0 { "PRI".into() } else { String::new() },
            default: r.get(3)?,
            extra: String::new(),
            collation: None,
        })
    })?;
    rows.collect()
}

/// `PRAGMA index_list` and `index_info` as MySQL indexes.
pub fn pragma_indexes(conn: &Connection, table: &str) -> Result<Vec<IndexDef>, rusqlite::Error> {
    let mut list = conn.prepare("SELECT name, \"unique\", origin FROM pragma_index_list(?1)")?;
    let indexes: Vec<(String, bool, String)> = list
        .query_map([table], |r| {
            Ok((r.get(0)?, r.get::<_, i64>(1)? == 1, r.get(2)?))
        })?
        .collect::<Result<_, _>>()?;
    let mut out = Vec::new();
    for (name, unique, origin) in indexes {
        let mut info = conn.prepare("SELECT name FROM pragma_index_info(?1) ORDER BY seqno")?;
        let columns = info
            .query_map([&name], |r| Ok((r.get::<_, String>(0)?, None)))?
            .collect::<Result<Vec<_>, _>>()?;
        let name = if origin == "pk" {
            "PRIMARY".to_string()
        } else {
            name.split_once("__")
                .map_or(name.clone(), |(_, n)| n.to_string())
        };
        out.push(IndexDef {
            name,
            unique,
            columns,
        });
    }
    Ok(out)
}

pub fn sqlite_version(conn: &Connection) -> Result<String, DbError> {
    conn.query_row("SELECT sqlite_version()", [], |r| r.get(0))
        .map_err(|e| crate::errors::map_error("sqlite", e))
}
