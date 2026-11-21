//! What `esc_js()` does after `_wp_specialchars()`.

use crate::{addslashes, stripslashes};

/// `esc_js()` after its `_wp_specialchars()` step: strip slashes, turn the single-quote references back into `'`,
/// drop carriage returns, add slashes, and write each newline as `\n`, so the text fits in a quoted JS string.
pub fn esc_js_after_specialchars(safe: &[u8]) -> Vec<u8> {
    let stripped = stripslashes(safe);
    let mut quoted = Vec::with_capacity(stripped.len());
    let mut i = 0;
    while i < stripped.len() {
        if stripped[i] == b'&'
            && let Some(len) = quote_reference(&stripped[i..])
        {
            quoted.push(b'\'');
            i += len;
            continue;
        }
        if stripped[i] != b'\r' {
            quoted.push(stripped[i]);
        }
        i += 1;
    }
    let slashed = addslashes(&quoted);
    let mut out = Vec::with_capacity(slashed.len() + 8);
    for &b in slashed.iter() {
        if b == b'\n' {
            out.extend_from_slice(b"\\n");
        } else {
            out.push(b);
        }
    }
    out
}

/// `&#(x)?0*(?(1)27|39);?`, ignoring case, at the start of `s`.
fn quote_reference(s: &[u8]) -> Option<usize> {
    if s.get(1) != Some(&b'#') {
        return None;
    }
    let hex = matches!(s.get(2), Some(b'x' | b'X'));
    let mut i = if hex { 3 } else { 2 };
    while s.get(i) == Some(&b'0') {
        i += 1;
    }
    let digits: &[u8] = if hex { b"27" } else { b"39" };
    if !s[i..].starts_with(digits) {
        return None;
    }
    i += 2;
    if s.get(i) == Some(&b';') {
        i += 1;
    }
    Some(i)
}
