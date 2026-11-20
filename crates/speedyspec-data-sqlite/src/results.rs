//! SQLite's rows as MySQL's: values, and the column descriptions mysqli gives.

use std::rc::Rc;

use rusqlite::types::ValueRef;
use speedyspec_data::{ColumnInfo, DbValue, Row};
use speedyspec_data_translate::{
    CatalogueColumn, CatalogueTable, IndexKind, Temporal, TypeFamily, column_key,
};

/// mysqli's column flags.
const NOT_NULL: u32 = 1;
const PRI_KEY: u32 = 2;
const UNIQUE_KEY: u32 = 4;
const MULTIPLE_KEY: u32 = 8;
const BLOB: u32 = 16;
const UNSIGNED: u32 = 32;
const BINARY: u32 = 128;
const AUTO_INCREMENT: u32 = 512;
const PART_KEY: u32 = 16384;
const NUM: u32 = 32768;

/// The charset ids mysqli reports: binary, and utf8mb4.
const CHARSET_BINARY: u16 = 63;
const CHARSET_UTF8MB4: u16 = 255;

/// MySQL's id for a collation, which mysqli reports as the column's charset.
fn collation_id(name: &str) -> u16 {
    match name {
        "utf8mb4_unicode_520_ci" => 246,
        "utf8mb4_unicode_ci" => 224,
        "utf8mb4_general_ci" => 45,
        "utf8mb4_bin" => 46,
        "utf8mb4_0900_bin" => 309,
        "utf8mb3_general_ci" | "utf8_general_ci" => 33,
        "utf8mb3_unicode_ci" | "utf8_unicode_ci" => 192,
        "utf8mb3_bin" | "utf8_bin" => 83,
        "latin1_swedish_ci" => 8,
        "latin1_general_ci" => 48,
        "latin1_bin" => 47,
        "ascii_general_ci" => 11,
        "binary" => CHARSET_BINARY,
        _ => CHARSET_UTF8MB4,
    }
}

/// Where a result column comes from: the table and its catalogue column, when it is a plain column.
pub struct Origin {
    pub table: String,
    pub column: String,
    pub catalogue: Option<(
        CatalogueColumn,
        Vec<speedyspec_data_translate::CatalogueIndex>,
    )>,
}

/// A value read from SQLite, as MySQL would return it for a column of `origin`'s type.
pub fn value(raw: ValueRef<'_>, origin: Option<&CatalogueColumn>) -> DbValue {
    let scale = origin.and_then(|c| match c.column_type.family() {
        TypeFamily::Decimal { scale } => Some(scale as usize),
        _ => None,
    });
    match raw {
        ValueRef::Null => DbValue::Null,
        ValueRef::Integer(i) => match scale {
            Some(s) => DbValue::Text(format!("{:.s$}", i as f64)),
            None => DbValue::Int(i),
        },
        ValueRef::Real(f) => match scale {
            // MySQL returns a DECIMAL with exactly its scale's digits.
            Some(s) => DbValue::Text(format!("{f:.s$}")),
            None => DbValue::Float(f),
        },
        ValueRef::Text(t) => match std::str::from_utf8(t) {
            Ok(s) => DbValue::Text(s.to_string()),
            Err(_) => DbValue::Bytes(t.to_vec()),
        },
        ValueRef::Blob(b) => DbValue::Bytes(b.to_vec()),
    }
}

/// mysqli's type name for a MySQL column type.
fn type_name(column: &CatalogueColumn) -> &'static str {
    let ty = &column.column_type;
    match ty.base.as_str() {
        "tinyint" => "MYSQL_TYPE_TINY",
        "smallint" => "MYSQL_TYPE_SHORT",
        "mediumint" => "MYSQL_TYPE_INT24",
        "int" => "MYSQL_TYPE_LONG",
        "bigint" => "MYSQL_TYPE_LONGLONG",
        "decimal" => "MYSQL_TYPE_NEWDECIMAL",
        "float" => "MYSQL_TYPE_FLOAT",
        "double" => "MYSQL_TYPE_DOUBLE",
        "bit" => "MYSQL_TYPE_BIT",
        "json" => "MYSQL_TYPE_JSON",
        "char" | "binary" | "enum" | "set" => "MYSQL_TYPE_STRING",
        "varchar" | "varbinary" => "MYSQL_TYPE_VAR_STRING",
        _ => match ty.family() {
            TypeFamily::Temporal(Temporal::Date) => "MYSQL_TYPE_DATE",
            TypeFamily::Temporal(Temporal::DateTime) => "MYSQL_TYPE_DATETIME",
            TypeFamily::Temporal(Temporal::Timestamp) => "MYSQL_TYPE_TIMESTAMP",
            TypeFamily::Temporal(Temporal::Time) => "MYSQL_TYPE_TIME",
            TypeFamily::Temporal(Temporal::Year) => "MYSQL_TYPE_YEAR",
            // MySQL reports TEXT and BLOB columns alike.
            _ => "MYSQL_TYPE_BLOB",
        },
    }
}

fn is_numeric(family: TypeFamily) -> bool {
    matches!(
        family,
        TypeFamily::Integer
            | TypeFamily::Decimal { .. }
            | TypeFamily::Float
            | TypeFamily::Temporal(Temporal::Year)
    )
}

fn arg(column: &CatalogueColumn, i: usize) -> Option<u64> {
    column.column_type.args.get(i).and_then(|a| a.parse().ok())
}

/// MySQL's column length: display width for numbers, bytes for strings (four per utf8mb4 character).
fn length(column: &CatalogueColumn) -> u64 {
    let ty = &column.column_type;
    let text = column.collation.is_some();
    let chars = |n: u64| if text { n * 4 } else { n };
    match ty.base.as_str() {
        "tinyint" => arg(column, 0).unwrap_or(4),
        "smallint" => arg(column, 0).unwrap_or(6),
        "mediumint" => arg(column, 0).unwrap_or(9),
        "int" => arg(column, 0).unwrap_or(11),
        "bigint" => 20,
        "decimal" => arg(column, 0).unwrap_or(10) + 2,
        "float" => 12,
        "double" => 22,
        "char" | "varchar" | "binary" | "varbinary" => chars(arg(column, 0).unwrap_or(1)),
        "tinytext" | "tinyblob" => 255,
        "text" | "blob" => 65535,
        "mediumtext" | "mediumblob" => 16_777_215,
        "longtext" | "longblob" | "json" => 4_294_967_295,
        "date" => 10,
        "datetime" | "timestamp" => 19,
        "time" => 10,
        "year" => 4,
        _ => 0,
    }
}

fn flags(column: &CatalogueColumn, indexes: &[speedyspec_data_translate::CatalogueIndex]) -> u32 {
    let ty = &column.column_type;
    let family = ty.family();
    let mut flags = 0;
    if !column.nullable {
        flags |= NOT_NULL;
    }
    flags |= match column_key(&column.name, indexes) {
        "PRI" => PRI_KEY,
        "UNI" => UNIQUE_KEY,
        "MUL" => MULTIPLE_KEY,
        _ => 0,
    };
    if indexes.iter().any(|i| {
        i.kind == IndexKind::Unique
            && i.parts.len() == 1
            && i.parts[0].0.eq_ignore_ascii_case(&column.name)
    }) {
        flags |= UNIQUE_KEY;
    }
    if ty.unsigned {
        flags |= UNSIGNED;
    }
    if column.extra == "auto_increment" {
        flags |= AUTO_INCREMENT;
    }
    if type_name(column) == "MYSQL_TYPE_BLOB" {
        flags |= BLOB;
    }
    if family == TypeFamily::Binary {
        flags |= BINARY;
    }
    if indexes.iter().any(|i| {
        i.parts
            .iter()
            .any(|(c, _)| c.eq_ignore_ascii_case(&column.name))
    }) {
        flags |= PART_KEY;
    }
    if is_numeric(family) {
        flags |= NUM;
    }
    flags
}

/// The description of an expression column, from the values it holds.
fn expression(name: &str, values: &[&DbValue], max_length: u64) -> ColumnInfo {
    let first = values.iter().find(|v| !matches!(v, DbValue::Null));
    let (sql_type, flags, charset, decimals) = match first {
        Some(DbValue::Int(_) | DbValue::UInt(_)) => {
            ("MYSQL_TYPE_LONGLONG", NUM | BINARY, CHARSET_BINARY, 0)
        }
        Some(DbValue::Float(_)) => ("MYSQL_TYPE_DOUBLE", NUM | BINARY, CHARSET_BINARY, 31),
        Some(DbValue::Bytes(_)) => ("MYSQL_TYPE_BLOB", BLOB | BINARY, CHARSET_BINARY, 0),
        Some(_) => ("MYSQL_TYPE_VAR_STRING", 0, CHARSET_UTF8MB4, 0),
        None => ("MYSQL_TYPE_NULL", NUM | BINARY, CHARSET_BINARY, 0),
    };
    ColumnInfo {
        name: name.to_string(),
        org_name: String::new(),
        table: String::new(),
        org_table: String::new(),
        sql_type: sql_type.to_string(),
        length: max_length,
        max_length,
        nullable: true,
        flags,
        decimals,
        charset,
    }
}

/// The columns of a result, described as mysqli describes MySQL's.
pub fn describe(names: &[String], origins: &[Option<Origin>], rows: &[Row]) -> Vec<ColumnInfo> {
    names
        .iter()
        .enumerate()
        .map(|(i, name)| {
            let values: Vec<&DbValue> = rows.iter().filter_map(|r| r.get_index(i)).collect();
            let max_length = values
                .iter()
                .map(|v| v.to_mysql_text().map_or(0, |t| t.len() as u64))
                .max()
                .unwrap_or(0);
            match &origins[i] {
                Some(Origin {
                    table,
                    column,
                    catalogue: Some((c, indexes)),
                }) => {
                    let family = c.column_type.family();
                    ColumnInfo {
                        name: name.clone(),
                        org_name: column.clone(),
                        // SQLite reports the origin table, not the alias the query used.
                        table: table.clone(),
                        org_table: table.clone(),
                        sql_type: type_name(c).to_string(),
                        length: length(c),
                        max_length,
                        nullable: c.nullable,
                        flags: flags(c, indexes),
                        decimals: match family {
                            TypeFamily::Decimal { scale } => scale as u8,
                            TypeFamily::Float => 31,
                            _ => 0,
                        },
                        charset: match &c.collation {
                            Some(collation)
                                if !is_numeric(family) && family != TypeFamily::Binary =>
                            {
                                collation_id(collation)
                            }
                            _ => CHARSET_BINARY,
                        },
                    }
                }
                Some(Origin {
                    table,
                    column,
                    catalogue: None,
                }) => ColumnInfo {
                    org_name: column.clone(),
                    table: table.clone(),
                    org_table: table.clone(),
                    ..expression(name, &values, max_length)
                },
                None => expression(name, &values, max_length),
            }
        })
        .collect()
}

/// The rows, sharing one list of column names.
pub fn rows_of(names: &[String], values: Vec<Vec<DbValue>>) -> Vec<Row> {
    let shared: Rc<[String]> = names.to_vec().into();
    values
        .into_iter()
        .map(|v| Row::new(shared.clone(), v))
        .collect()
}

/// The origin's catalogue column and the table's indexes, if the catalogue has the table.
pub fn catalogue_column(
    table: &CatalogueTable,
    column: &str,
) -> Option<(
    CatalogueColumn,
    Vec<speedyspec_data_translate::CatalogueIndex>,
)> {
    table
        .columns
        .iter()
        .find(|c| c.name.eq_ignore_ascii_case(column))
        .map(|c| (c.clone(), table.indexes.clone()))
}
