//! Scrubbing, noncharacters and `check_invalid_utf8`.

use std::borrow::Cow;

use speedyspec_wp_utf8::{check_invalid_utf8, has_noncharacters, scrub_utf8};

const FFFD: &str = "\u{FFFD}";

#[test]
fn scrub_borrows_valid_input() {
    assert!(matches!(scrub_utf8("héllo".as_bytes()), Cow::Borrowed(_)));
    assert!(matches!(scrub_utf8(b""), Cow::Borrowed(_)));
}

#[test]
fn scrub_replaces_each_maximal_subpart() {
    assert_eq!(&*scrub_utf8(b"\xF0\x9F\x98"), FFFD.as_bytes());
    assert_eq!(&*scrub_utf8(b"\xED\xA0\x80"), FFFD.repeat(3).as_bytes());
    assert_eq!(&*scrub_utf8(b"\xC0\xAF"), FFFD.repeat(2).as_bytes());
    assert_eq!(&*scrub_utf8(b"a\xE2\x82"), format!("a{FFFD}").as_bytes());
    assert_eq!(&*scrub_utf8(b"a\xFFb\xFF"), format!("a{FFFD}b{FFFD}").as_bytes());
}

#[test]
fn noncharacters_match_bytes_even_in_invalid_text() {
    // WordPress 7.1.5 matches the UTF-8 byte sequences directly, so malformed bytes elsewhere don't hide them.
    for b in [&b"\xEF\xB7\x90\xFF"[..], b"\xFF\xEF\xBF\xBE", b"\xE2\xEF\xBF\xBE", b"\xF0\x9F\xBF\xBE", b"\xF4\x8F\xBF\xBF"] {
        assert!(has_noncharacters(b), "{b:x?}");
    }
    for b in [&b"\xF4\x9F\xBF\xBF"[..], b"\xEF\xB7\x8F", b"\xEF\xB7\xB0", b"abc", b""] {
        assert!(!has_noncharacters(b), "{b:x?}");
    }
}

#[test]
fn noncharacters_found_in_valid_text() {
    for s in ["\u{FDD0}", "a\u{FFFE}", "\u{1FFFF}", "x\u{10FFFE}y"] {
        assert!(has_noncharacters(s.as_bytes()), "{s:?}");
    }
    for s in ["", "abc", "\u{FDCF}\u{FDF0}\u{FFFD}", "😀"] {
        assert!(!has_noncharacters(s.as_bytes()), "{s:?}");
    }
}

#[test]
fn check_invalid_utf8_rules() {
    assert!(matches!(check_invalid_utf8("ok é".as_bytes(), false), Cow::Borrowed(b) if b == "ok é".as_bytes()));
    assert_eq!(&*check_invalid_utf8(b"a\xFFb", false), b"");
    // WordPress's wp_check_invalid_utf8 scrubs (U+FFFD) when asked to strip.
    assert_eq!(&*check_invalid_utf8(b"a\xFFb", true), format!("a{FFFD}b").as_bytes());
}
