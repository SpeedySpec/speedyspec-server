//! Values in and out as mysqli gives them (Adapter): parameters sent in PostgreSQL's text format, so the server
//! coerces `'5'` as MySQL does, and results decoded from binary by type into the strings and numbers mysqli returns.

use std::error::Error;

use bytes::BytesMut;
use postgres::types::{Format, FromSql, IsNull, ToSql, Type};
use speedyspec_data::{ColumnInfo, DbError, DbValue, DialectKind};

type BoxError = Box<dyn Error + Sync + Send>;

/// A parameter, always in text format.
#[derive(Debug)]
pub struct PgParam<'a>(pub &'a DbValue);

impl ToSql for PgParam<'_> {
    fn to_sql(&self, ty: &Type, out: &mut BytesMut) -> Result<IsNull, BoxError> {
        match self.0 {
            DbValue::Null => return Ok(IsNull::Yes),
            DbValue::Bytes(b) if *ty == Type::BYTEA => {
                out.extend_from_slice(b"\\x");
                for byte in b {
                    out.extend_from_slice(format!("{byte:02x}").as_bytes());
                }
            }
            DbValue::Bytes(b) => match std::str::from_utf8(b) {
                Ok(s) => out.extend_from_slice(s.as_bytes()),
                Err(_) => {
                    return Err(
                        format!("bytes that are not UTF-8 for a {} parameter", ty.name()).into(),
                    );
                }
            },
            other => out.extend_from_slice(&other.to_mysql_text().unwrap_or_default()),
        }
        Ok(IsNull::No)
    }

    fn accepts(_ty: &Type) -> bool {
        true
    }

    fn encode_format(&self, _ty: &Type) -> Format {
        Format::Text
    }

    postgres::types::to_sql_checked!();
}

/// A result value, as mysqli would give the matching MySQL value.
#[derive(Debug, Clone, PartialEq)]
pub struct PgCell(pub DbValue);

impl<'a> FromSql<'a> for PgCell {
    fn from_sql(ty: &Type, raw: &'a [u8]) -> Result<PgCell, BoxError> {
        decode(ty, raw)
            .map(PgCell)
            .map_err(|e| e.to_string().into())
    }

    fn from_sql_null(_ty: &Type) -> Result<PgCell, BoxError> {
        Ok(PgCell(DbValue::Null))
    }

    fn accepts(_ty: &Type) -> bool {
        true
    }
}

fn short(raw: &[u8]) -> DbError {
    DbError::Engine {
        engine: "postgres".into(),
        code: None,
        message: format!("a result value of {} bytes is too short", raw.len()),
    }
}

fn int<const N: usize>(raw: &[u8]) -> Result<[u8; N], DbError> {
    raw.get(..N)
        .and_then(|b| b.try_into().ok())
        .ok_or_else(|| short(raw))
}

/// The civil date `days` after 2000-01-01.
fn civil(days: i64) -> (i64, u32, u32) {
    let z = days + 10_957 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

fn clock(micros: i64) -> String {
    let secs = micros.div_euclid(1_000_000);
    let frac = micros.rem_euclid(1_000_000);
    let text = format!("{:02}:{:02}:{:02}", secs / 3600, secs / 60 % 60, secs % 60);
    if frac == 0 {
        text
    } else {
        format!("{text}.{frac:06}")
    }
}

fn numeric(raw: &[u8]) -> Result<String, DbError> {
    let n = i16::from_be_bytes(int(raw)?) as usize;
    let weight = i16::from_be_bytes(int(&raw[2..])?) as i64;
    let sign = u16::from_be_bytes(int(&raw[4..])?);
    let dscale = u16::from_be_bytes(int(&raw[6..])?) as usize;
    if sign == 0xC000 {
        return Ok("NaN".into());
    }
    let digits: Vec<i64> = (0..n)
        .map(|i| int::<2>(&raw[8 + 2 * i..]).map(|b| i64::from(i16::from_be_bytes(b))))
        .collect::<Result<_, _>>()?;
    let group = |w: i64| -> i64 {
        let i = weight - w;
        if i >= 0 && (i as usize) < digits.len() {
            digits[i as usize]
        } else {
            0
        }
    };
    let mut whole = String::new();
    for w in (0..=weight.max(0)).rev() {
        if whole.is_empty() {
            if group(w) != 0 || w == 0 {
                whole = group(w).to_string();
            }
        } else {
            whole.push_str(&format!("{:04}", group(w)));
        }
    }
    if whole.is_empty() {
        whole.push('0');
    }
    let mut fraction = String::new();
    let mut w = -1;
    while fraction.len() < dscale {
        fraction.push_str(&format!("{:04}", group(w)));
        w -= 1;
    }
    fraction.truncate(dscale);
    let zero = whole == "0" && fraction.chars().all(|c| c == '0');
    let sign = if sign == 0x4000 && !zero { "-" } else { "" };
    Ok(if dscale == 0 {
        format!("{sign}{whole}")
    } else {
        format!("{sign}{whole}.{fraction}")
    })
}

fn utf8(raw: &[u8]) -> DbValue {
    match std::str::from_utf8(raw) {
        Ok(s) => DbValue::Text(s.to_string()),
        Err(_) => DbValue::Bytes(raw.to_vec()),
    }
}

/// A binary result value of type `ty`, as mysqli returns the matching MySQL value.
pub fn decode(ty: &Type, raw: &[u8]) -> Result<DbValue, DbError> {
    Ok(match *ty {
        Type::BOOL => DbValue::Int(i64::from(raw.first().copied().unwrap_or(0) != 0)),
        Type::INT2 => DbValue::Int(i64::from(i16::from_be_bytes(int(raw)?))),
        Type::INT4 => DbValue::Int(i64::from(i32::from_be_bytes(int(raw)?))),
        Type::OID => DbValue::Int(i64::from(u32::from_be_bytes(int(raw)?))),
        Type::INT8 => DbValue::Int(i64::from_be_bytes(int(raw)?)),
        Type::FLOAT4 => DbValue::Float(f64::from(f32::from_be_bytes(int(raw)?))),
        Type::FLOAT8 => DbValue::Float(f64::from_be_bytes(int(raw)?)),
        Type::NUMERIC => DbValue::Text(numeric(raw)?),
        Type::TEXT | Type::VARCHAR | Type::BPCHAR | Type::NAME | Type::UNKNOWN | Type::JSON => {
            utf8(raw)
        }
        Type::BYTEA => DbValue::Bytes(raw.to_vec()),
        Type::DATE => {
            let days = i32::from_be_bytes(int(raw)?);
            DbValue::Text(match days {
                i32::MIN => "0000-00-00".into(),
                i32::MAX => "9999-12-31".into(),
                d => {
                    let (y, m, d) = civil(i64::from(d));
                    format!("{y:04}-{m:02}-{d:02}")
                }
            })
        }
        Type::TIME => DbValue::Text(clock(i64::from_be_bytes(int(raw)?))),
        Type::TIMESTAMP | Type::TIMESTAMPTZ => {
            let micros = i64::from_be_bytes(int(raw)?);
            DbValue::Text(match micros {
                i64::MIN => "0000-00-00 00:00:00".into(),
                i64::MAX => "9999-12-31 23:59:59".into(),
                us => {
                    let (y, m, d) = civil(us.div_euclid(86_400_000_000));
                    format!(
                        "{y:04}-{m:02}-{d:02} {}",
                        clock(us.rem_euclid(86_400_000_000))
                    )
                }
            })
        }
        Type::UUID => {
            let hex: String = raw.iter().map(|b| format!("{b:02x}")).collect();
            if hex.len() != 32 {
                return Err(short(raw));
            }
            DbValue::Text(format!(
                "{}-{}-{}-{}-{}",
                &hex[..8],
                &hex[8..12],
                &hex[12..16],
                &hex[16..20],
                &hex[20..]
            ))
        }
        ref other => {
            return Err(DbError::Unsupported {
                construct: format!("result column of PostgreSQL type {}", other.name()),
                dialect: DialectKind::Postgres,
            });
        }
    })
}

#[allow(dead_code)] // the store's result columns (Task 8)
/// The column as mysqli describes the matching MySQL column, in the type names `MySqlStore` gives.
pub fn mysql_column_info(
    col: &postgres::Column,
    table: Option<&str>,
    values: &[DbValue],
    index: usize,
) -> ColumnInfo {
    let ty = col.type_();
    let (sql_type, binary, numeric) = match *ty {
        Type::INT8 => ("MYSQL_TYPE_LONGLONG", false, true),
        Type::INT4 => ("MYSQL_TYPE_LONG", false, true),
        Type::INT2 => ("MYSQL_TYPE_SHORT", false, true),
        Type::BOOL => ("MYSQL_TYPE_TINY", false, true),
        Type::NUMERIC => ("MYSQL_TYPE_NEWDECIMAL", false, true),
        Type::FLOAT8 => ("MYSQL_TYPE_DOUBLE", false, true),
        Type::FLOAT4 => ("MYSQL_TYPE_FLOAT", false, true),
        Type::TEXT => ("MYSQL_TYPE_BLOB", false, false),
        Type::VARCHAR => ("MYSQL_TYPE_VAR_STRING", false, false),
        Type::BPCHAR => ("MYSQL_TYPE_STRING", false, false),
        Type::TIMESTAMP | Type::TIMESTAMPTZ => ("MYSQL_TYPE_DATETIME", false, false),
        Type::DATE => ("MYSQL_TYPE_DATE", false, false),
        Type::TIME => ("MYSQL_TYPE_TIME", false, false),
        Type::BYTEA => ("MYSQL_TYPE_BLOB", true, false),
        _ => ("MYSQL_TYPE_VAR_STRING", false, false),
    };
    let max_length = values
        .iter()
        .map(|v| v.to_mysql_text().map_or(0, |t| t.len() as u64))
        .max()
        .unwrap_or(0);
    let _ = index;
    ColumnInfo {
        name: col.name().to_string(),
        org_name: col.name().to_string(),
        table: table.unwrap_or_default().to_string(),
        org_table: table.unwrap_or_default().to_string(),
        sql_type: sql_type.to_string(),
        length: max_length,
        max_length,
        nullable: true,
        flags: if numeric { 32768 } else { 0 } | if sql_type == "MYSQL_TYPE_BLOB" { 16 } else { 0 },
        decimals: 0,
        charset: if binary || numeric { 63 } else { 246 },
    }
}
