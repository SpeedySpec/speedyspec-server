//! A result column described as mysqli describes the MySQL column it comes from: length, flags, charset, origin.

use speedyspec_data::{ColumnInfo, Rows};
use speedyspec_data_translate::{
    CatalogueColumn, CatalogueIndex, CatalogueTable, IndexKind, Temporal, TypeFamily, column_key,
};

use crate::exec::Origins;

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

fn collation_id(name: &str) -> u16 {
    match name {
        "utf8mb4_unicode_520_ci" => 246,
        "utf8mb4_unicode_ci" => 224,
        "utf8mb4_general_ci" => 45,
        "utf8mb4_bin" => 46,
        "utf8mb4_0900_ai_ci" => 255,
        "utf8mb3_general_ci" | "utf8_general_ci" => 33,
        "utf8mb3_unicode_ci" | "utf8_unicode_ci" => 192,
        "utf8mb3_bin" | "utf8_bin" => 83,
        "latin1_swedish_ci" => 8,
        "latin1_general_ci" => 48,
        "latin1_bin" => 47,
        "ascii_general_ci" => 11,
        _ => 255,
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

fn arg(c: &CatalogueColumn, i: usize) -> Option<u64> {
    c.column_type.args.get(i).and_then(|a| a.parse().ok())
}

fn length(c: &CatalogueColumn) -> u64 {
    let chars = |n: u64| if c.collation.is_some() { n * 4 } else { n };
    match c.column_type.base.as_str() {
        "tinyint" => arg(c, 0).unwrap_or(4),
        "smallint" => arg(c, 0).unwrap_or(6),
        "mediumint" => arg(c, 0).unwrap_or(9),
        "int" => arg(c, 0).unwrap_or(11),
        "bigint" => 20,
        "decimal" => arg(c, 0).unwrap_or(10) + 2,
        "float" => 12,
        "double" => 22,
        "char" | "varchar" | "binary" | "varbinary" => chars(arg(c, 0).unwrap_or(1)),
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

fn type_name(c: &CatalogueColumn) -> &'static str {
    match c.column_type.base.as_str() {
        "tinyint" => "MYSQL_TYPE_TINY",
        "smallint" => "MYSQL_TYPE_SHORT",
        "mediumint" => "MYSQL_TYPE_INT24",
        "int" => "MYSQL_TYPE_LONG",
        "bigint" => "MYSQL_TYPE_LONGLONG",
        "decimal" => "MYSQL_TYPE_NEWDECIMAL",
        "float" => "MYSQL_TYPE_FLOAT",
        "double" => "MYSQL_TYPE_DOUBLE",
        "json" => "MYSQL_TYPE_JSON",
        "char" | "binary" | "enum" | "set" => "MYSQL_TYPE_STRING",
        "varchar" | "varbinary" => "MYSQL_TYPE_VAR_STRING",
        "date" => "MYSQL_TYPE_DATE",
        "datetime" => "MYSQL_TYPE_DATETIME",
        "timestamp" => "MYSQL_TYPE_TIMESTAMP",
        "time" => "MYSQL_TYPE_TIME",
        "year" => "MYSQL_TYPE_YEAR",
        _ => "MYSQL_TYPE_BLOB",
    }
}

fn flags(c: &CatalogueColumn, indexes: &[CatalogueIndex]) -> u32 {
    let family = c.column_type.family();
    let mut f = if c.nullable { 0 } else { NOT_NULL };
    f |= match column_key(&c.name, indexes) {
        "PRI" => PRI_KEY,
        "UNI" => UNIQUE_KEY,
        "MUL" => MULTIPLE_KEY,
        _ => 0,
    };
    if indexes.iter().any(|i| {
        i.kind == IndexKind::Unique
            && i.parts.len() == 1
            && i.parts[0].0.eq_ignore_ascii_case(&c.name)
    }) {
        f |= UNIQUE_KEY;
    }
    if indexes
        .iter()
        .any(|i| i.parts.iter().any(|(p, _)| p.eq_ignore_ascii_case(&c.name)))
    {
        f |= PART_KEY;
    }
    if c.column_type.unsigned {
        f |= UNSIGNED;
    }
    if c.extra == "auto_increment" {
        f |= AUTO_INCREMENT;
    }
    if type_name(c) == "MYSQL_TYPE_BLOB" {
        f |= BLOB;
    }
    if family == TypeFamily::Binary {
        f |= BINARY;
    }
    if is_numeric(family) {
        f |= NUM;
    }
    f
}

/// Fills each plain column's description from its table's catalogue entry; `resolve` names an origin (table,
/// column), `catalogue` reads a table's entry.
pub(crate) fn describe(
    rows: &mut Rows,
    origins: &Origins,
    resolve: &dyn Fn(u32, i16) -> Option<(String, String)>,
    catalogue: &dyn Fn(&str) -> Option<CatalogueTable>,
) {
    for (info, origin) in rows.columns.iter_mut().zip(origins) {
        let Some((oid, attnum)) = origin else {
            continue;
        };
        let Some((table, column)) = resolve(*oid, *attnum) else {
            continue;
        };
        info.table = table.clone();
        info.org_table = table.clone();
        info.org_name = column.clone();
        let Some(entry) = catalogue(&table) else {
            continue;
        };
        let Some(c) = entry
            .columns
            .iter()
            .find(|c| c.name.eq_ignore_ascii_case(&column))
        else {
            continue;
        };
        apply(info, c, &entry.indexes);
    }
}

fn apply(info: &mut ColumnInfo, c: &CatalogueColumn, indexes: &[CatalogueIndex]) {
    let family = c.column_type.family();
    info.sql_type = type_name(c).to_string();
    info.length = length(c);
    info.nullable = c.nullable;
    info.flags = flags(c, indexes);
    info.decimals = match family {
        TypeFamily::Decimal { scale } => scale as u8,
        TypeFamily::Float => 31,
        _ => 0,
    };
    info.charset = match &c.collation {
        Some(k) if !is_numeric(family) && family != TypeFamily::Binary => collation_id(k),
        _ => 63,
    };
}
