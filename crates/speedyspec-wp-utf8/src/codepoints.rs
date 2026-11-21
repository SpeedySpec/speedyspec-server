//! `_wp_utf8_codepoint_count` and `_wp_utf8_codepoint_span`.

use crate::scan::scan_utf8;

/// Counts code points in `text` from `byte_offset` over at most `max_byte_length` bytes; each invalid span counts as
/// one, as `_wp_utf8_codepoint_count` does.
pub fn codepoint_count(text: &[u8], byte_offset: usize, max_byte_length: usize) -> usize {
    let end = text.len();
    if byte_offset >= end {
        return 0;
    }
    let max = (end - byte_offset).min(max_byte_length);
    let mut count = 0;
    let mut at = byte_offset;
    while at < end && at - byte_offset < max {
        let s = scan_utf8(text, at, Some(max - (at - byte_offset)), None);
        count += s.code_points + usize::from(s.invalid_length > 0);
        at = s.at + s.invalid_length;
    }
    count
}

/// The bytes spanned by up to `max_code_points` code points from `byte_offset`, and how many were found, as
/// `_wp_utf8_codepoint_span` returns and writes to `$found_code_points`.
pub fn codepoint_span(text: &[u8], byte_offset: usize, max_code_points: usize) -> (usize, usize) {
    let end = text.len();
    let mut at = byte_offset;
    let mut found = 0;
    while at < end && found < max_code_points {
        let s = scan_utf8(text, at, None, Some(max_code_points - found));
        found += s.code_points;
        at = s.at;
        if s.invalid_length != 0 && found < max_code_points {
            found += 1;
            at += s.invalid_length;
        }
    }
    (at - byte_offset, found)
}
