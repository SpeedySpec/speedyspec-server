//! Query text that is not UTF-8: PHP strings are bytes, the parser reads `str`.
//!
//! Each byte that is not part of valid UTF-8 is carried through the parser as a character of the Supplementary
//! Private Use Area-B (`U+10FF00 + byte`). A string literal holding one becomes a `?` bound to the literal's exact
//! bytes, so the store receives them unchanged; such a byte anywhere else is refused.

use std::ops::ControlFlow;

use speedyspec_data::ast::{Expr, Value, visit_expressions_mut};
use speedyspec_data::{DbError, DbValue, DialectKind, Statement};
use speedyspec_data_query::{mysql_text, parse_one};

const BASE: u32 = 0x10FF00;

fn is_marker(c: char) -> bool {
    (BASE..=BASE + 0xFF).contains(&(c as u32))
}

fn to_bytes(s: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len());
    for c in s.chars() {
        if is_marker(c) {
            out.push((c as u32 - BASE) as u8);
        } else {
            let mut buf = [0u8; 4];
            out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
        }
    }
    out
}

/// Parses query text from PHP.
pub(crate) fn parse_bytes(sql: &[u8]) -> Result<Statement, DbError> {
    if let Ok(text) = std::str::from_utf8(sql) {
        return parse_one(text);
    }
    let mut mapped = String::with_capacity(sql.len() + 8);
    for chunk in sql.utf8_chunks() {
        mapped.push_str(chunk.valid());
        for &b in chunk.invalid() {
            mapped.push(char::from_u32(BASE + u32::from(b)).expect("in the private use area"));
        }
    }
    let (mut tree, mut params) = parse_one(&mapped)?.into_parts();
    let _ = visit_expressions_mut(&mut tree, |expr| {
        if let Expr::Value(v) = expr {
            match &v.value {
                Value::SingleQuotedString(s)
                | Value::DoubleQuotedString(s)
                | Value::NationalStringLiteral(s)
                    if s.chars().any(is_marker) =>
                {
                    params.push(DbValue::Bytes(to_bytes(s)));
                    v.value = Value::Placeholder("?".into());
                }
                _ => {}
            }
        }
        ControlFlow::<()>::Continue(())
    });
    if mysql_text(&tree).chars().any(is_marker) {
        return Err(DbError::Unsupported {
            construct: "bytes that are not UTF-8 outside a string value".into(),
            dialect: DialectKind::MySql,
        });
    }
    Ok(Statement::new(tree, params))
}
