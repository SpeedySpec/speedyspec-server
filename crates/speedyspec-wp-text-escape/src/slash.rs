//! PHP's `addslashes()`/`stripslashes()`, and WordPress's slashing helpers built on them.

use std::borrow::Cow;

use memchr::{memchr, memchr3};
use speedyspec_hooks::Value;

/// PHP's `addslashes()`: a backslash before `'`, `"` and `\`, and NUL as `\0`.
pub fn addslashes(text: &[u8]) -> Cow<'_, [u8]> {
    let needs = |b: u8| matches!(b, b'\'' | b'"' | b'\\' | 0);
    if memchr3(b'\'', b'"', b'\\', text).is_none() && memchr(0, text).is_none() {
        return Cow::Borrowed(text);
    }
    let mut out = Vec::with_capacity(text.len() + 8);
    for &b in text {
        if needs(b) {
            out.push(b'\\');
            out.push(if b == 0 { b'0' } else { b });
        } else {
            out.push(b);
        }
    }
    Cow::Owned(out)
}

/// PHP's `stripslashes()`: `\0` becomes NUL, `\x` becomes `x`, and a trailing lone `\` is dropped.
pub fn stripslashes(text: &[u8]) -> Cow<'_, [u8]> {
    if memchr(b'\\', text).is_none() {
        return Cow::Borrowed(text);
    }
    let mut out = Vec::with_capacity(text.len());
    let mut i = 0;
    while i < text.len() {
        if text[i] == b'\\' {
            if let Some(&next) = text.get(i + 1) {
                out.push(if next == b'0' { 0 } else { next });
            }
            i += 2;
        } else {
            out.push(text[i]);
            i += 1;
        }
    }
    Cow::Owned(out)
}

/// WordPress's `backslashit()`: two backslashes before a leading digit, and one before every ASCII letter, so the
/// text is safe in a date format string.
pub fn backslashit(text: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len() * 2 + 2);
    if text.first().is_some_and(u8::is_ascii_digit) {
        out.extend_from_slice(b"\\\\");
    }
    for &b in text {
        if b.is_ascii_alphabetic() {
            out.push(b'\\');
        }
        out.push(b);
    }
    out
}

/// A PHP string from bytes: `Str` when they are UTF-8, `Bytes` otherwise.
pub(crate) fn php_string(bytes: Vec<u8>) -> Value {
    match String::from_utf8(bytes) {
        Ok(s) => Value::Str(s),
        Err(e) => Value::Bytes(e.into_bytes()),
    }
}

fn map_strings(value: &Value, f: &dyn Fn(&[u8]) -> Cow<'_, [u8]>) -> Value {
    match value {
        Value::Str(s) => match f(s.as_bytes()) {
            Cow::Borrowed(_) => value.clone(),
            Cow::Owned(bytes) => php_string(bytes),
        },
        Value::Bytes(b) => match f(b) {
            Cow::Borrowed(_) => value.clone(),
            Cow::Owned(bytes) => php_string(bytes),
        },
        Value::Array(pairs) => Value::Array(
            pairs
                .iter()
                .map(|(key, item)| (key.clone(), map_strings(item, f)))
                .collect(),
        ),
        other => other.clone(),
    }
}

/// WordPress's `wp_slash()`: `addslashes()` on a string, recursively on an array (keys kept), anything else as it is.
pub fn wp_slash_value(value: &Value) -> Value {
    map_strings(value, &addslashes)
}

/// WordPress's `wp_unslash()` (`stripslashes_deep()`) over strings and arrays. An object is returned as it is: the
/// PHP adapter walks object properties.
pub fn wp_unslash_value(value: &Value) -> Value {
    map_strings(value, &stripslashes)
}
