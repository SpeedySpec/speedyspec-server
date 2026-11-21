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

/// `wp_has_noncharacters` as WordPress runs it on PHP with PCRE: a `/u` match, so invalid UTF-8 is never a match.
pub fn has_noncharacters(bytes: &[u8]) -> bool {
    if !is_valid_utf8(bytes) {
        return false;
    }
    // Every noncharacter is encoded with a lead byte of EF or F0..F4; text without them has none.
    if memchr::memchr3(0xEF, 0xF0, 0xF1, bytes).is_none() && memchr::memchr3(0xF2, 0xF3, 0xF4, bytes).is_none() {
        return false;
    }
    has_noncharacters_reference(bytes)
}

/// `_wp_has_noncharacters_fallback`: scans past invalid sequences, so noncharacters in invalid text are found.
pub fn has_noncharacters_reference(bytes: &[u8]) -> bool {
    let mut at = 0;
    while at < bytes.len() {
        let s = scan_utf8(bytes, at, None, None);
        if s.has_noncharacters {
            return true;
        }
        at = s.at + s.invalid_length;
        if s.invalid_length == 0 {
            break;
        }
    }
    false
}

/// The core of WordPress's `wp_check_invalid_utf8` after its charset check: valid input comes back unchanged;
/// invalid input becomes empty, or is scrubbed (U+FFFD) when `strip` is true.
pub fn check_invalid_utf8(bytes: &[u8], strip: bool) -> Cow<'_, [u8]> {
    if bytes.is_empty() || is_valid_utf8(bytes) {
        return Cow::Borrowed(bytes);
    }
    if strip { scrub_utf8(bytes) } else { Cow::Borrowed(&[]) }
}
