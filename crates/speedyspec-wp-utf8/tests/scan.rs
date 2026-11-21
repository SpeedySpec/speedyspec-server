//! `_wp_scan_utf8` and validation. Expected values were taken from WordPress's own `compat-utf8.php` run on PHP.

use speedyspec_wp_utf8::{Scan, is_valid_utf8, is_valid_utf8_reference, scan_utf8};

fn scan(bytes: &[u8], max_bytes: Option<usize>, max_code_points: Option<usize>) -> Scan {
    scan_utf8(bytes, 0, max_bytes, max_code_points)
}

#[test]
fn scan_counts_code_points_and_stops_at_invalid() {
    let s = scan(b"a\xC3\xA9\xFFb", None, None);
    assert_eq!(
        (s.code_points, s.at, s.invalid_length, s.has_noncharacters),
        (2, 3, 1, false)
    );
}

#[test]
fn scan_finishes_a_sequence_that_crosses_max_bytes() {
    // WordPress reads the rest of a multibyte sequence even past `max_bytes`.
    let s = scan("é€".as_bytes(), Some(3), None);
    assert_eq!((s.code_points, s.at, s.invalid_length), (2, 5, 0));
}

#[test]
fn scan_stops_at_max_code_points() {
    let s = scan("aéb".as_bytes(), None, Some(2));
    assert_eq!((s.code_points, s.at), (2, 3));
}

#[test]
fn scan_flags_noncharacters() {
    let s = scan("\u{FDD0}".as_bytes(), None, None);
    assert!(s.has_noncharacters);
    assert!(s.noncharacters_tested);
}

#[test]
fn two_byte_text_never_tests_noncharacters() {
    // PHP's `|=` only runs for EF-led three-byte and for four-byte sequences; elsewhere the by-reference value stays
    // the boolean `false` rather than becoming the integer 0.
    assert!(!scan("é".as_bytes(), None, None).noncharacters_tested);
    assert!(scan("😀".as_bytes(), None, None).noncharacters_tested);
}

#[test]
fn scan_reports_maximal_subpart_length() {
    // E2 82 then ASCII: a valid two-byte prefix of a three-byte sequence.
    let s = scan(b"\xE2\x82a", None, None);
    assert_eq!((s.at, s.invalid_length), (0, 2));
    let s = scan(b"\xF0\x9F\x98", None, None);
    assert_eq!((s.at, s.invalid_length), (0, 3));
}

#[test]
fn empty_and_nul_are_valid() {
    for b in [&b""[..], b"a\0b"] {
        assert!(is_valid_utf8(b));
        assert!(is_valid_utf8_reference(b));
    }
}

fn hex(line: &str) -> Vec<u8> {
    (0..line.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&line[i..i + 2], 16).unwrap())
        .collect()
}

#[test]
fn fast_path_agrees_with_reference() {
    let mut buf = Vec::with_capacity(3);
    for len in 0..=3usize {
        let total = 1u32 << (8 * len);
        for n in 0..total {
            buf.clear();
            for k in 0..len {
                buf.push((n >> (8 * k)) as u8);
            }
            assert_eq!(
                is_valid_utf8(&buf),
                is_valid_utf8_reference(&buf),
                "{buf:02x?}"
            );
        }
    }
    let cases = include_str!("data/utf8-cases.txt");
    for line in cases
        .lines()
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
    {
        let b = hex(line.trim());
        assert_eq!(is_valid_utf8(&b), is_valid_utf8_reference(&b), "{line}");
    }
}
