//! Scrubbing, noncharacter detection and `check_invalid_utf8`.

use std::borrow::Cow;

use crate::scan::scan_utf8;
use crate::validate::is_valid_utf8;

const REPLACEMENT: &[u8] = "\u{FFFD}".as_bytes();

/// Replaces each maximal subpart of an invalid sequence with U+FFFD, as `wp_scrub_utf8` and
/// `_wp_scrub_utf8_fallback` do. Valid input comes back borrowed.
pub fn scrub_utf8(bytes: &[u8]) -> Cow<'_, [u8]> {
    if is_valid_utf8(bytes) {
        return Cow::Borrowed(bytes);
    }
    let mut out = Vec::with_capacity(bytes.len() + 8);
    let mut at = 0;
    loop {
        let s = scan_utf8(bytes, at, None, None);
        if s.at >= bytes.len() {
            out.extend_from_slice(&bytes[at..s.at - s.invalid_length]);
            return Cow::Owned(out);
        }
        out.extend_from_slice(&bytes[at..s.at]);
        out.extend_from_slice(REPLACEMENT);
        at = s.at + s.invalid_length;
    }
}

/// `wp_has_noncharacters`, as WordPress 7.1.5 matches it: the noncharacters' UTF-8 byte sequences, anywhere in the
/// bytes, so malformed UTF-8 elsewhere does not hide them.
///
/// The pattern is `\xEF(?:\xB7[\x90-\xAF]|\xBF[\xBE\xBF])` or
/// `(?:\xF0[\x9F\xAF\xBF]|[\xF1-\xF3][\x8F\x9F\xAF\xBF]|\xF4\x8F)\xBF[\xBE\xBF]`.
pub fn has_noncharacters(bytes: &[u8]) -> bool {
    let at = |i: usize| bytes.get(i).copied();
    let mut from = 0;
    // Every match starts with EF or F0..F4; nothing else can begin one.
    while let Some(off) = bytes[from..].iter().position(|b| matches!(b, 0xEF..=0xF4)) {
        let i = from + off;
        let b2 = at(i + 1);
        let tail = |j: usize| at(j) == Some(0xBF) && matches!(at(j + 1), Some(0xBE | 0xBF));
        let hit = match bytes[i] {
            0xEF => match b2 {
                Some(0xB7) => matches!(at(i + 2), Some(0x90..=0xAF)),
                Some(0xBF) => matches!(at(i + 2), Some(0xBE | 0xBF)),
                _ => false,
            },
            0xF0 => matches!(b2, Some(0x9F | 0xAF | 0xBF)) && tail(i + 2),
            0xF1..=0xF3 => matches!(b2, Some(0x8F | 0x9F | 0xAF | 0xBF)) && tail(i + 2),
            _ => b2 == Some(0x8F) && tail(i + 2),
        };
        if hit {
            return true;
        }
        from = i + 1;
    }
    false
}

/// The core of WordPress's `wp_check_invalid_utf8` after its charset check: valid input comes back unchanged;
/// invalid input becomes empty, or is scrubbed (U+FFFD) when `strip` is true.
pub fn check_invalid_utf8(bytes: &[u8], strip: bool) -> Cow<'_, [u8]> {
    if bytes.is_empty() || is_valid_utf8(bytes) {
        return Cow::Borrowed(bytes);
    }
    if strip {
        scrub_utf8(bytes)
    } else {
        Cow::Borrowed(&[])
    }
}
