//! Headers as a PHP array: assigning an existing name keeps its position and takes the new value.

pub(crate) fn assign(headers: &mut Vec<(String, String)>, name: String, value: String) {
    match headers.iter_mut().find(|(k, _)| *k == name) {
        Some(slot) => slot.1 = value,
        None => headers.push((name, value)),
    }
}

/// PHP's `trim()` default characters.
pub(crate) fn php_trim(s: &[u8]) -> &[u8] {
    let is = |b: &u8| matches!(b, b' ' | b'\t' | b'\n' | b'\r' | b'\0' | b'\x0B');
    let start = s.iter().position(|b| !is(b)).unwrap_or(s.len());
    let end = s.iter().rposition(|b| !is(b)).map_or(start, |i| i + 1);
    &s[start..end]
}

/// Header text as a Rust string. Header values that are not UTF-8 are rare (the header declares the charset); they
/// are kept with replacement characters.
pub(crate) fn text(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}
