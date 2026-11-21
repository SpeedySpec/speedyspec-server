//! `_wp_utf8_codepoint_count` and `_wp_utf8_codepoint_span`.

use speedyspec_wp_utf8::{codepoint_count, codepoint_span};

#[test]
fn codepoint_count_respects_offset_and_length() {
    assert_eq!(codepoint_count("aé€b".as_bytes(), 1, 5), 2);
    assert_eq!(codepoint_count("aé€b".as_bytes(), 0, usize::MAX), 4);
}

#[test]
fn invalid_spans_count_as_one_each() {
    assert_eq!(codepoint_count(b"a\xFF\xFEb", 0, usize::MAX), 4);
    assert_eq!(codepoint_count(b"\xE2\x82a", 0, usize::MAX), 2);
}

#[test]
fn offset_past_end_counts_nothing() {
    assert_eq!(codepoint_count(b"abc", 9, usize::MAX), 0);
}

#[test]
fn codepoint_span_returns_bytes_and_found() {
    assert_eq!(codepoint_span("aé€b".as_bytes(), 0, 3), (6, 3));
    assert_eq!(codepoint_span(b"a\xFFb", 0, 2), (2, 2));
    assert_eq!(codepoint_span(b"abc", 1, 10), (2, 2));
}
