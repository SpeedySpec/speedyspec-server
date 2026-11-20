//! `wpdb`'s charset rules: ASCII, charset upgrades, column lengths and `strip_invalid_text`.

use std::sync::OnceLock;

use regex::bytes::Regex;
use speedyspec_hooks::Value;

use crate::escape::{PlaceholderEscape, string_value};
use crate::{ColLength, Field, LengthKind, Prepare, ServerCaps, WpDbError};

/// What `strip_invalid_text` needs beyond the fields: the escape `prepare()` uses and the connection's charset.
pub struct StripContext<'a> {
    pub escape: &'a PlaceholderEscape,
    pub connection_charset: &'a str,
}

/// Runs one `SELECT CONVERT(..)` statement and gives its first row as a list (`get_row(.., ARRAY_N)`).
pub type Convert<'a> = dyn FnMut(&[u8]) -> Result<Vec<Option<Vec<u8>>>, WpDbError> + 'a;

/// WordPress's charset rules (pure functions).
pub struct CharsetRules;

struct Strip {
    key: String,
    value: Value,
    charset: Option<String>,
    length: Option<ColLength>,
    check_ascii: bool,
    db: bool,
}

/// The length of the valid UTF-8 sequence at the start of `b`, by WordPress's regex (3-byte limit unless `mb4`).
fn utf8_sequence(b: &[u8], mb4: bool) -> Option<usize> {
    let cont = |i: usize, lo: u8, hi: u8| b.get(i).is_some_and(|&c| (lo..=hi).contains(&c));
    let c = *b.first()?;
    match c {
        0x00..=0x7F => Some(1),
        0xC2..=0xDF if cont(1, 0x80, 0xBF) => Some(2),
        0xE0 if cont(1, 0xA0, 0xBF) && cont(2, 0x80, 0xBF) => Some(3),
        0xE1..=0xEC | 0xEE..=0xEF if cont(1, 0x80, 0xBF) && cont(2, 0x80, 0xBF) => Some(3),
        0xED if cont(1, 0x80, 0x9F) && cont(2, 0x80, 0xBF) => Some(3),
        0xF0 if mb4 && cont(1, 0x90, 0xBF) && cont(2, 0x80, 0xBF) && cont(3, 0x80, 0xBF) => Some(4),
        0xF1..=0xF3 if mb4 && cont(1, 0x80, 0xBF) && cont(2, 0x80, 0xBF) && cont(3, 0x80, 0xBF) => {
            Some(4)
        }
        0xF4 if mb4 && cont(1, 0x80, 0x8F) && cont(2, 0x80, 0xBF) && cont(3, 0x80, 0xBF) => Some(4),
        _ => None,
    }
}

fn bytes_of(v: &Value) -> Option<&[u8]> {
    match v {
        Value::Str(s) => Some(s.as_bytes()),
        Value::Bytes(b) => Some(b),
        _ => None,
    }
}

impl CharsetRules {
    /// `wpdb::check_ascii`.
    pub fn check_ascii(input: &[u8]) -> bool {
        input.is_ascii()
    }

    /// `wpdb::determine_charset` with a connection.
    pub fn determine_charset(charset: &str, collate: &str, caps: &ServerCaps) -> (String, String) {
        let mut charset = charset.to_string();
        let mut collate = collate.to_string();
        if charset == "utf8" {
            charset = "utf8mb4".into();
        }
        if charset == "utf8mb4" {
            if collate.is_empty() || collate == "0" || collate == "utf8_general_ci" {
                collate = "utf8mb4_unicode_ci".into();
            } else {
                collate = collate.replace("utf8_", "utf8mb4_");
            }
        }
        if caps.has_cap("utf8mb4_520") && collate == "utf8mb4_unicode_ci" {
            collate = "utf8mb4_unicode_520_ci".into();
        }
        (charset, collate)
    }

    /// `get_col_length`'s table from a column's MySQL type.
    pub fn col_length(column_type: &str) -> Option<ColLength> {
        let mut parts = column_type.splitn(2, '(');
        let ty = parts.next().unwrap_or("").to_ascii_lowercase();
        let length = parts
            .next()
            .map(|l| l.trim_matches(')'))
            .filter(|l| !l.is_empty());
        let declared = || {
            length
                .and_then(|l| {
                    let digits: String = l.chars().take_while(char::is_ascii_digit).collect();
                    digits.parse::<u64>().ok()
                })
                .unwrap_or(0)
        };
        let (kind, length) = match ty.as_str() {
            "char" | "varchar" => (LengthKind::Char, declared()),
            "binary" | "varbinary" => (LengthKind::Byte, declared()),
            "tinyblob" | "tinytext" => (LengthKind::Byte, 255),
            "blob" | "text" => (LengthKind::Byte, 65_535),
            "mediumblob" | "mediumtext" => (LengthKind::Byte, 16_777_215),
            "longblob" | "longtext" => (LengthKind::Byte, 4_294_967_295),
            _ => return None,
        };
        Some(ColLength { kind, length })
    }

    /// `wpdb::strip_invalid_text` over a helper's fields (each keyed by its name).
    pub fn strip_invalid_text(
        fields: &mut [Field],
        cx: &StripContext,
        convert: &mut Convert,
    ) -> Result<(), WpDbError> {
        let mut items: Vec<Strip> = fields
            .iter()
            .map(|f| Strip {
                key: f.name.clone(),
                value: f.value.clone(),
                charset: f.charset.clone(),
                length: f.length,
                check_ascii: true,
                db: false,
            })
            .collect();
        strip(&mut items, cx, convert)?;
        for (field, item) in fields.iter_mut().zip(items) {
            field.value = item.value;
        }
        Ok(())
    }

    /// `wpdb::strip_invalid_text_from_query`, with the charset of the query's table (or the connection's) given.
    pub fn strip_invalid_text_from_query(
        query: &[u8],
        charset: &str,
        cx: &StripContext,
        convert: &mut Convert,
    ) -> Result<Vec<u8>, WpDbError> {
        static SKIP: OnceLock<Regex> = OnceLock::new();
        let trimmed_start = query
            .iter()
            .position(|b| !b"\r\n\t (".contains(b))
            .unwrap_or(query.len());
        let skip = SKIP
            .get_or_init(|| Regex::new(r"(?i-u)^(?:SHOW|DESCRIBE|DESC|EXPLAIN|CREATE)\s").unwrap());
        if skip.is_match(&query[trimmed_start..]) || charset == "binary" {
            return Ok(query.to_vec());
        }
        let mut items = vec![Strip {
            key: "0".into(),
            value: Value::Bytes(query.to_vec()),
            charset: Some(charset.to_string()),
            length: None,
            check_ascii: false,
            db: false,
        }];
        strip(&mut items, cx, convert)?;
        Ok(bytes_of(&items[0].value).unwrap_or_default().to_vec())
    }
}

fn set_bytes(original: &Value, bytes: Vec<u8>) -> Value {
    match original {
        Value::Str(_) => string_value(bytes),
        _ => Value::Bytes(bytes),
    }
}

fn strip(items: &mut [Strip], cx: &StripContext, convert: &mut Convert) -> Result<(), WpDbError> {
    let mut db_check = false;
    for item in items.iter_mut() {
        let (length, mut by_byte) = match item.length {
            Some(l) => (Some(l.length), l.kind == LengthKind::Byte),
            None => (None, false),
        };
        let Some(charset) = item.charset.clone() else {
            continue;
        };
        let Some(value) = bytes_of(&item.value).map(<[u8]>::to_vec) else {
            continue;
        };
        let mut value = value;
        let mut needs_validation = true;
        if charset == "latin1" || (item.check_ascii && value.is_ascii()) {
            by_byte = true;
            needs_validation = false;
        }
        if by_byte {
            if let Some(l) = length.filter(|l| value.len() as u64 > *l) {
                value.truncate(l as usize);
            }
            if !needs_validation {
                item.value = set_bytes(&item.value, value);
                continue;
            }
        }
        if matches!(charset.as_str(), "utf8" | "utf8mb3" | "utf8mb4") {
            let mb4 = charset == "utf8mb4";
            let mut clean = Vec::with_capacity(value.len());
            let mut i = 0;
            while i < value.len() {
                match utf8_sequence(&value[i..], mb4) {
                    Some(n) => {
                        clean.extend_from_slice(&value[i..i + n]);
                        i += n;
                    }
                    None => i += 1,
                }
            }
            if let Some(l) = length {
                let text = String::from_utf8_lossy(&clean).into_owned();
                if text.chars().count() as u64 > l {
                    clean = text
                        .chars()
                        .take(l as usize)
                        .collect::<String>()
                        .into_bytes();
                }
            }
            item.value = set_bytes(&item.value, clean);
            continue;
        }
        item.value = set_bytes(&item.value, value);
        item.db = true;
        db_check = true;
    }
    if !db_check {
        return Ok(());
    }
    let prepare = Prepare {
        escape: cx.escape,
        allow_unsafe_unquoted_parameters: true,
    };
    let mut selects: Vec<(usize, Vec<u8>)> = Vec::new();
    for (index, item) in items.iter_mut().enumerate() {
        if !item.db {
            continue;
        }
        let byte = item.length.is_some_and(|l| l.kind == LengthKind::Byte);
        let charset = if byte {
            "binary".to_string()
        } else {
            item.charset.clone().unwrap_or_default()
        };
        let conn = cx.connection_charset;
        let value = [item.value.clone()];
        let query = match item.length {
            Some(l) => prepare.prepare(
                Some(
                    format!(
                        "CONVERT( LEFT( CONVERT( %s USING {charset} ), {} ) USING {conn} )",
                        l.length
                    )
                    .as_bytes(),
                ),
                &value,
            ),
            None if charset != "binary" => prepare.prepare(
                Some(format!("CONVERT( CONVERT( %s USING {charset} ) USING {conn} )").as_bytes()),
                &value,
            ),
            None => {
                item.db = false;
                continue;
            }
        };
        item.db = false;
        if let Some(sql) = query.sql.filter(|s| !s.is_empty()) {
            let mut sql = sql;
            sql.extend_from_slice(format!(" AS x_{}", item.key).as_bytes());
            selects.push((index, sql));
        }
    }
    let mut statement = b"SELECT ".to_vec();
    statement.extend_from_slice(
        &selects
            .iter()
            .map(|(_, s)| s.clone())
            .collect::<Vec<_>>()
            .join(&b", "[..]),
    );
    let row = convert(&statement)?;
    if row.is_empty() {
        return Err(WpDbError::InvalidData(
            "Could not strip invalid text.".into(),
        ));
    }
    for ((index, _), value) in selects.iter().zip(row) {
        if let Some(value) = value {
            items[*index].value = set_bytes(&items[*index].value, value);
        }
    }
    Ok(())
}
