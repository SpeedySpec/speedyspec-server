//! PHP's `addslashes()`/`stripslashes()`, and WordPress's slashing helpers built on them.

use std::borrow::Cow;

use memchr::{memchr, memchr3};
use speedyspec_hooks::Value;

/// PHP's `addslashes()`: a backslash before `'`, `"` and `\`, and NUL as `\0`.
pub fn addslashes(text: &[u8]) -> Cow<'_, [u8]> {
    const SPECIAL: [bool; 256] = {
        let mut t = [false; 256];
        t[b'\'' as usize] = true;
        t[b'"' as usize] = true;
        t[b'\\' as usize] = true;
        t[0] = true;
        t
    };
    if memchr(0, text).is_none() {
        // No NUL: the three other bytes, found with SIMD.
        let Some(first) = memchr3(b'\'', b'"', b'\\', text) else {
            return Cow::Borrowed(text);
        };
        let mut out = Vec::with_capacity(text.len() + text.len() / 16 + 8);
        let mut copied = 0;
        let mut at = first;
        loop {
            out.extend_from_slice(&text[copied..at]);
            out.extend_from_slice(&[b'\\', text[at]]);
            copied = at + 1;
            match memchr3(b'\'', b'"', b'\\', &text[copied..]) {
                Some(next) => at = copied + next,
                None => break,
            }
        }
        out.extend_from_slice(&text[copied..]);
        return Cow::Owned(out);
    }
    let Some(first) = text.iter().position(|&b| SPECIAL[usize::from(b)]) else {
        return Cow::Borrowed(text);
    };
    let mut out = Vec::with_capacity(text.len() + text.len() / 16 + 8);
    out.extend_from_slice(&text[..first]);
    let mut copied = first;
    for (i, &b) in text.iter().enumerate().skip(first) {
        if SPECIAL[usize::from(b)] {
            out.extend_from_slice(&text[copied..i]);
            out.extend_from_slice(&[b'\\', if b == 0 { b'0' } else { b }]);
            copied = i + 1;
        }
    }
    out.extend_from_slice(&text[copied..]);
    Cow::Owned(out)
}

/// PHP's `stripslashes()`: `\0` becomes NUL, `\x` becomes `x`, and a trailing lone `\` is dropped.
pub fn stripslashes(text: &[u8]) -> Cow<'_, [u8]> {
    let Some(mut at) = memchr(b'\\', text) else {
        return Cow::Borrowed(text);
    };
    let mut out = Vec::with_capacity(text.len());
    let mut copied = 0;
    loop {
        out.extend_from_slice(&text[copied..at]);
        if let Some(&next) = text.get(at + 1) {
            out.push(if next == b'0' { 0 } else { next });
        }
        copied = (at + 2).min(text.len());
        match memchr(b'\\', &text[copied..]) {
            Some(found) => at = copied + found,
            None => break,
        }
    }
    out.extend_from_slice(&text[copied..]);
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
