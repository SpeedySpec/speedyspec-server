//! Escaping: mysqli's `real_escape_string`, `addslashes`, `esc_like`, identifiers, and the placeholder escape.

use std::collections::hash_map::RandomState;
use std::hash::{BuildHasher, Hasher};
use std::time::{SystemTime, UNIX_EPOCH};

use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;
use speedyspec_hooks::Value;

use crate::php::{is_scalar, to_bytes};

/// The per-instance string that stands for `%` in prepared SQL until the query runs (`wpdb::placeholder_escape`).
#[derive(Debug, Clone)]
pub struct PlaceholderEscape {
    placeholder: String,
}

fn random_u64() -> u64 {
    let mut h = RandomState::new().build_hasher();
    h.write_u128(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0),
    );
    h.finish()
}

impl PlaceholderEscape {
    /// `salt` is `AUTH_SALT`; without it a random number stands in, as WordPress does.
    pub fn new(salt: Option<&str>) -> PlaceholderEscape {
        let salt = match salt.filter(|s| !s.is_empty()) {
            Some(s) => s.to_string(),
            None => (random_u64() % 2_147_483_648).to_string(),
        };
        // `uniqid($salt, true)`: the salt, 13 hex digits of the time, and a random fraction.
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        let unique = format!(
            "{salt}{:08x}{:05x}.{:08}",
            now.as_secs(),
            now.subsec_micros(),
            random_u64() % 100_000_000
        );
        let mut mac =
            Hmac::<Sha256>::new_from_slice(salt.as_bytes()).expect("HMAC takes any key length");
        mac.update(unique.as_bytes());
        let hex: String = mac
            .finalize()
            .into_bytes()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        PlaceholderEscape {
            placeholder: format!("{{{hex}}}"),
        }
    }

    pub fn placeholder(&self) -> &str {
        &self.placeholder
    }

    /// `%` → the placeholder.
    pub fn add(&self, query: &str) -> String {
        query.replace('%', &self.placeholder)
    }

    /// The placeholder → `%`.
    pub fn remove(&self, query: &str) -> String {
        query.replace(&self.placeholder, "%")
    }

    pub fn add_bytes(&self, query: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(query.len());
        for &b in query {
            if b == b'%' {
                out.extend_from_slice(self.placeholder.as_bytes());
            } else {
                out.push(b);
            }
        }
        out
    }

    pub fn remove_bytes(&self, query: &[u8]) -> Vec<u8> {
        let needle = self.placeholder.as_bytes();
        let mut out = Vec::with_capacity(query.len());
        let mut i = 0;
        while i < query.len() {
            if query[i..].starts_with(needle) {
                out.push(b'%');
                i += needle.len();
            } else {
                out.push(query[i]);
                i += 1;
            }
        }
        out
    }
}

/// mysqli's `real_escape_string` for utf8mb4.
pub fn real_escape(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() + 8);
    for &b in data {
        match b {
            0 => out.extend_from_slice(b"\\0"),
            b'\n' => out.extend_from_slice(b"\\n"),
            b'\r' => out.extend_from_slice(b"\\r"),
            b'\\' => out.extend_from_slice(b"\\\\"),
            b'\'' => out.extend_from_slice(b"\\'"),
            b'"' => out.extend_from_slice(b"\\\""),
            0x1a => out.extend_from_slice(b"\\Z"),
            other => out.push(other),
        }
    }
    out
}

/// `addslashes()`.
pub fn weak_escape(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() + 4);
    for &b in data {
        match b {
            0 => out.extend_from_slice(b"\\0"),
            b'\'' | b'"' | b'\\' => {
                out.push(b'\\');
                out.push(b);
            }
            other => out.push(other),
        }
    }
    out
}

/// `addcslashes($text, '_%\\')`.
pub fn esc_like(text: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len() + 4);
    for &b in text {
        if matches!(b, b'_' | b'%' | b'\\') {
            out.push(b'\\');
        }
        out.push(b);
    }
    out
}

/// `wpdb::quote_identifier`.
pub fn quote_identifier(identifier: &str) -> String {
    format!("`{}`", identifier.replace('`', "``"))
}

/// A PHP string value from bytes: `Str` when UTF-8, else `Bytes`.
pub(crate) fn string_value(bytes: Vec<u8>) -> Value {
    match String::from_utf8(bytes) {
        Ok(s) => Value::Str(s),
        Err(e) => Value::Bytes(e.into_bytes()),
    }
}

/// `wpdb::_real_escape` (with a connection): non-scalars give `''`; then the placeholder escape.
pub(crate) fn real_escape_value(data: &Value, escape: &PlaceholderEscape) -> Vec<u8> {
    if !is_scalar(data) {
        return Vec::new();
    }
    escape.add_bytes(&real_escape(&to_bytes(data).unwrap_or_default()))
}

/// `wpdb::_escape`: arrays recursively, other values through `_real_escape`.
pub fn escape(data: &Value, escape_with: &PlaceholderEscape) -> Value {
    match data {
        Value::Array(items) => Value::Array(
            items
                .iter()
                .map(|(k, v)| {
                    let v = match v {
                        Value::Array(_) => escape(v, escape_with),
                        _ => string_value(real_escape_value(v, escape_with)),
                    };
                    (k.clone(), v)
                })
                .collect(),
        ),
        other => string_value(real_escape_value(other, escape_with)),
    }
}
