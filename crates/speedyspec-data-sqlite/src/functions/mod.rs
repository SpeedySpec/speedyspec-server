//! MySQL's functions, as SQLite connection functions with MySQL's semantics.

mod dates;
mod misc;
mod strings;

use std::sync::{Arc, Mutex};

use rusqlite::Connection;
use rusqlite::functions::{Context, FunctionFlags};
use rusqlite::types::ValueRef;
use speedyspec_data_translate::SessionVars;

/// Registers the collation and every function on `conn`.
pub fn register(conn: &Connection, session: Arc<Mutex<SessionVars>>) -> rusqlite::Result<()> {
    crate::collation::register(conn)?;
    dates::register(conn)?;
    strings::register(conn)?;
    misc::register(conn, session)?;
    Ok(())
}

pub(crate) fn deterministic() -> FunctionFlags {
    FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC
}

pub(crate) fn volatile() -> FunctionFlags {
    FunctionFlags::SQLITE_UTF8
}

/// An argument as text (numbers as MySQL prints them); `None` for NULL.
pub(crate) fn text(ctx: &Context<'_>, i: usize) -> Option<String> {
    match ctx.get_raw(i) {
        ValueRef::Null => None,
        ValueRef::Integer(n) => Some(n.to_string()),
        ValueRef::Real(f) => Some(if f.fract() == 0.0 && f.abs() < 1e15 {
            format!("{}", f as i64)
        } else {
            f.to_string()
        }),
        ValueRef::Text(b) | ValueRef::Blob(b) => Some(String::from_utf8_lossy(b).into_owned()),
    }
}

pub(crate) fn is_blob(ctx: &Context<'_>, i: usize) -> bool {
    matches!(ctx.get_raw(i), ValueRef::Blob(_))
}

/// MySQL's numeric reading of a string: its longest numeric prefix (`'12abc'` is 12, `'abc'` is 0).
pub(crate) fn numeric_prefix(s: &str) -> f64 {
    let s = s.trim_start();
    let mut end = 0;
    let bytes = s.as_bytes();
    if end < bytes.len() && (bytes[end] == b'-' || bytes[end] == b'+') {
        end += 1;
    }
    while end < bytes.len() && bytes[end].is_ascii_digit() {
        end += 1;
    }
    if end < bytes.len() && bytes[end] == b'.' {
        end += 1;
        while end < bytes.len() && bytes[end].is_ascii_digit() {
            end += 1;
        }
    }
    s[..end].parse().unwrap_or(0.0)
}

/// An argument as a number; `None` for NULL.
pub(crate) fn number(ctx: &Context<'_>, i: usize) -> Option<f64> {
    match ctx.get_raw(i) {
        ValueRef::Null => None,
        ValueRef::Integer(n) => Some(n as f64),
        ValueRef::Real(f) => Some(f),
        ValueRef::Text(b) | ValueRef::Blob(b) => Some(numeric_prefix(&String::from_utf8_lossy(b))),
    }
}

pub(crate) fn int(ctx: &Context<'_>, i: usize) -> Option<i64> {
    match ctx.get_raw(i) {
        ValueRef::Integer(n) => Some(n),
        _ => number(ctx, i).map(|f| f.trunc() as i64),
    }
}

pub(crate) fn user_error(message: impl Into<String>) -> rusqlite::Error {
    rusqlite::Error::UserFunctionError(message.into().into())
}
