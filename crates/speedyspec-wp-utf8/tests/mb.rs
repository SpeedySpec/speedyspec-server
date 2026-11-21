//! `_mb_substr`, `_mb_strlen`, `_is_utf8_charset` and the Latin-1 fallbacks. Cases from WordPress's
//! `tests/phpunit/tests/compat/mbSubstr.php` and `mbStrlen.php`, with expected values from PHP's own `mb_substr`
//! (UTF-8) and `substr` (8bit).

use speedyspec_wp_utf8::{is_utf8_charset, mb_strlen, mb_substr, utf8_decode_latin1, utf8_encode_latin1};

#[rustfmt::skip]
const SUBSTR: &[(&[u8], i64, Option<i64>, &[u8], &[u8])] = &[
    (b"\xd0\xb1\xd0\xb0\xd0\xb1\xd0\xb0", 0, Some(3), b"\xd0\xb1\xd0\xb0\xd0\xb1", b"\xd0\xb1\xd0"),
    (b"\xd0\xb1\xd0\xb0\xd0\xb1\xd0\xb0", 0, Some(-1), b"\xd0\xb1\xd0\xb0\xd0\xb1", b"\xd0\xb1\xd0\xb0\xd0\xb1\xd0"),
    (b"\xd0\xb1\xd0\xb0\xd0\xb1\xd0\xb0", 1, None, b"\xd0\xb0\xd0\xb1\xd0\xb0", b"\xb1\xd0\xb0\xd0\xb1\xd0\xb0"),
    (b"\xd0\xb1\xd0\xb0\xd0\xb1\xd0\xb0", -3, None, b"\xd0\xb0\xd0\xb1\xd0\xb0", b"\xb1\xd0\xb0"),
    (b"\xd0\xb1\xd0\xb0\xd0\xb1\xd0\xb0", -3, Some(2), b"\xd0\xb0\xd0\xb1", b"\xb1\xd0"),
    (b"\xd0\xb1\xd0\xb0\xd0\xb1\xd0\xb0", -2, Some(1), b"\xd0\xb1", b"\xd0"),
    (b"\xd0\xb1\xd0\xb0\xd0\xb1\xd0\xb0", 30, Some(1), b"", b""),
    (b"\xd0\xb1\xd0\xb0\xd0\xb1\xd0\xb0", 15, Some(-30), b"", b""),
    (b"\xd0\xb1\xd0\xb0\xd0\xb1\xd0\xb0", -5, Some(-5), b"", b""),
    (b"\xd0\xb1\xd0\xb0\xd0\xb1\xd0\xb0", 5, Some(-3), b"", b""),
    (b"\xd0\xb1\xd0\xb0\xd0\xb1\xd0\xb0", -3, Some(5), b"\xd0\xb0\xd0\xb1\xd0\xb0", b"\xb1\xd0\xb0"),
    (b"\x49\x20\x61\x6d\x20\x79\x6f\x75\x72\x20\xd0\xb1\xd0\xb0\xd0\xb1\xd0\xb0", 0, Some(11), b"\x49\x20\x61\x6d\x20\x79\x6f\x75\x72\x20\xd0\xb1", b"\x49\x20\x61\x6d\x20\x79\x6f\x75\x72\x20\xd0"),
    (b"\x68\xc3\xa9\x6c\x6c\x6f\x20\x77\xc3\xb6\x72\x6c\x64", -5, Some(-2), b"\x77\xc3\xb6\x72", b"\xc3\xb6\x72"),
    (b"\xe6\x97\xa5\xe6\x9c\xac\xe8\xaa\x9e\xe3\x83\x86\xe3\x82\xad\xe3\x82\xb9\xe3\x83\x88\xe3\x81\xa7\xe3\x81\x99\xe3\x80\x82\x30\x31\x32\x33\x34\xef\xbc\x95\xef\xbc\x96\xef\xbc\x97\xef\xbc\x98\xef\xbc\x99\xe3\x80\x82", 2, Some(7), b"\xe8\xaa\x9e\xe3\x83\x86\xe3\x82\xad\xe3\x82\xb9\xe3\x83\x88\xe3\x81\xa7\xe3\x81\x99", b"\xa5\xe6\x9c\xac\xe8\xaa\x9e"),
    (b"\x41\x42\x43\x44\x45\x46", 3, None, b"\x44\x45\x46", b"\x44\x45\x46"),
    (b"\x41\x42\x43\x44\x45\x46", -2, None, b"\x45\x46", b"\x45\x46"),
    (b"\x41\x42\x43\x44\x45\x46", 1, Some(-10), b"", b""),
];

#[test]
fn mb_substr_matches_php_utf8_and_8bit() {
    for (s, start, len, utf8, bytes) in SUBSTR {
        assert_eq!(&*mb_substr(s, *start, *len, true), *utf8, "utf8 {s:x?} {start} {len:?}");
        assert_eq!(&*mb_substr(s, *start, *len, false), *bytes, "8bit {s:x?} {start} {len:?}");
    }
}

#[test]
fn mb_substr_invalid_bytes_count_as_one() {
    assert_eq!(&*mb_substr(b"a\xFFb", 1, Some(1), true), b"\xFF");
}

#[test]
fn mb_strlen_counts_code_points_or_bytes() {
    for (s, n) in [("баба", 4), ("𝟜𝟜", 2), ("1²３𝟜", 4)] {
        assert_eq!(mb_strlen(s.as_bytes(), true), n);
        assert_eq!(mb_strlen(s.as_bytes(), false), s.len());
    }
    assert_eq!(mb_strlen(b"a\xFF\xFEb", true), 4);
}

#[test]
fn utf8_charset_names() {
    for c in ["UTF-8", "utf-8", "Utf8", "UTF8"] {
        assert!(is_utf8_charset(c.as_bytes()), "{c}");
    }
    for c in ["utf-16", "latin-1", "", "UTF-8 "] {
        assert!(!is_utf8_charset(c.as_bytes()), "{c}");
    }
}

#[test]
fn latin1_roundtrip() {
    assert_eq!(utf8_encode_latin1(b"caf\xE9"), "café".as_bytes());
    assert_eq!(utf8_decode_latin1(&utf8_encode_latin1(b"caf\xE9")), b"caf\xE9");
    assert_eq!(utf8_decode_latin1("€".as_bytes()), b"?");
    assert_eq!(utf8_decode_latin1(b"a\xFFb"), b"a?b");
}
