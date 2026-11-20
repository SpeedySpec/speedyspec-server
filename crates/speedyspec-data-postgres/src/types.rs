//! MySQL column types as PostgreSQL's, and MySQL 8.4's own spelling of a MySQL type.

use speedyspec_data::{DbError, DialectKind};
use speedyspec_data_translate::{MySqlType, Temporal, TypeFamily};
use sqlparser::ast::{CharacterLength, DataType, ExactNumberInfo, TimezoneInfo};

fn unsupported(construct: String) -> DbError {
    DbError::Unsupported {
        construct,
        dialect: DialectKind::Postgres,
    }
}

fn length(ty: &MySqlType, default: u64) -> Option<CharacterLength> {
    let n = ty
        .args
        .first()
        .and_then(|a| a.parse().ok())
        .unwrap_or(default);
    Some(CharacterLength::IntegerLength {
        length: n,
        unit: None,
    })
}

/// The PostgreSQL type for a MySQL type.
pub(crate) fn pg_type(ty: &MySqlType) -> Result<DataType, DbError> {
    Ok(match (ty.base.as_str(), ty.unsigned) {
        ("tinyint", _) | ("smallint", false) | ("year", _) => DataType::SmallInt(None),
        ("mediumint", _) | ("int", false) | ("smallint", true) => DataType::Integer(None),
        ("int", true) | ("bigint", _) => DataType::BigInt(None),
        ("decimal", _) => {
            let p = ty.args.first().and_then(|a| a.parse().ok()).unwrap_or(10);
            let s = ty.args.get(1).and_then(|a| a.parse().ok()).unwrap_or(0);
            DataType::Numeric(ExactNumberInfo::PrecisionAndScale(p, s))
        }
        ("float", _) => DataType::Real,
        ("double", _) => DataType::DoublePrecision,
        ("char", _) => DataType::Char(length(ty, 1)),
        ("varchar", _) => DataType::Varchar(length(ty, 255)),
        ("tinytext" | "text" | "mediumtext" | "longtext" | "json", _) => DataType::Text,
        ("tinyblob" | "blob" | "mediumblob" | "longblob" | "binary" | "varbinary", _) => {
            DataType::Bytea
        }
        ("datetime" | "timestamp", _) => DataType::Timestamp(Some(0), TimezoneInfo::None),
        ("date", _) => DataType::Date,
        ("time", _) => DataType::Time(Some(0), TimezoneInfo::None),
        ("enum", _) => {
            let longest = ty
                .args
                .iter()
                .map(|a| a.trim_matches(|c| c == '\'' || c == '"').chars().count() as u64)
                .max()
                .unwrap_or(1);
            DataType::Varchar(Some(CharacterLength::IntegerLength {
                length: longest,
                unit: None,
            }))
        }
        _ => return Err(unsupported(format!("the MySQL type {}", ty.display()))),
    })
}

/// Whether the type holds text, which takes a collation.
pub(crate) fn is_text(ty: &MySqlType) -> bool {
    matches!(ty.family(), TypeFamily::Text | TypeFamily::Enum) && ty.base != "json"
}

pub(crate) fn is_temporal(ty: &MySqlType) -> bool {
    matches!(
        ty.family(),
        TypeFamily::Temporal(Temporal::Date | Temporal::DateTime | Temporal::Timestamp)
    )
}

/// MySQL 8.4's `SHOW COLUMNS` spelling of a type as written (`bigint(20) unsigned` → `bigint unsigned`).
pub fn mysql_display(mysql_type: &str) -> String {
    MySqlType::parse(mysql_type).map_or_else(|_| mysql_type.to_ascii_lowercase(), |t| t.display())
}
