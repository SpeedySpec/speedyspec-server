//! `_is_utf8_charset`, `_mb_substr` and `_mb_strlen`: WordPress's mbstring-free string helpers.

use std::borrow::Cow;

use crate::codepoints::{codepoint_count, codepoint_span};

/// Whether `charset` names UTF-8 (`UTF-8` or `UTF8`, any case), as `_is_utf8_charset` decides.
pub fn is_utf8_charset(charset: &[u8]) -> bool {
    charset.eq_ignore_ascii_case(b"UTF-8") || charset.eq_ignore_ascii_case(b"UTF8")
}

/// PHP 8's `substr( $s, $start, $length )`.
pub fn php_substr(s: &[u8], start: i64, length: Option<i64>) -> &[u8] {
    let len = s.len() as i64;
    if start > len {
        return &[];
    }
    let from = if start < 0 {
        (len + start).max(0)
    } else {
        start
    };
    let to = match length {
        None => len,
        Some(l) if l < 0 => len + l,
        Some(l) => from.saturating_add(l).min(len),
    };
    if to <= from {
        &[]
    } else {
        &s[from as usize..to as usize]
    }
}

/// `_mb_substr`: code points when `utf8` (the charset is UTF-8), bytes (`substr`) otherwise. Each invalid span
/// counts as one code point.
pub fn mb_substr(s: &[u8], start: i64, length: Option<i64>, utf8: bool) -> Cow<'_, [u8]> {
    if !utf8 {
        return Cow::Borrowed(php_substr(s, start, length));
    }
    let negative_length = length.is_some_and(|l| l < 0);
    let total = if start < 0 || negative_length {
        codepoint_count(s, 0, usize::MAX) as i64
    } else {
        0
    };
    let start = if start < 0 {
        (total + start).max(0)
    } else {
        start
    };
    let (starting_byte, _) = codepoint_span(s, 0, start as usize);
    let length = match length {
        Some(l) if l < 0 => Some((total - start + l).max(0)),
        other => other,
    };
    let byte_length = match length {
        Some(l) => codepoint_span(s, starting_byte, l as usize).0 as i64,
        None => (s.len() - starting_byte) as i64,
    };
    Cow::Borrowed(php_substr(s, starting_byte as i64, Some(byte_length)))
}

/// `_mb_strlen`: code points when `utf8`, bytes otherwise.
pub fn mb_strlen(s: &[u8], utf8: bool) -> usize {
    if utf8 {
        codepoint_count(s, 0, usize::MAX)
    } else {
        s.len()
    }
}
