use std::borrow::Cow;

use serde_json::json;
use speedyspec_wp_kses_lexical::{html_error, no_null, stripslashes};
use speedyspec_wp_kses_testkit::{PhpOracle, bytes, from_bytes};

#[test]
fn no_null_removes_controls() {
    assert_eq!(&*no_null(b"a\x00b\\0c", true), b"ab\\0c");
    assert_eq!(&*no_null(b"a\\\\00b", false), b"ab");
    assert_eq!(&*no_null(b"t\tn\nr\r\x0B\x0C\x1Fx", true), b"t\tn\nr\rx");
    assert!(matches!(no_null(b"clean", true), Cow::Borrowed(_)));
    assert!(matches!(no_null(b"clean", false), Cow::Borrowed(_)));
}

#[test]
fn html_error_drops_first_attribute() {
    assert_eq!(html_error(b"\"a b\" c"), b"c");
    assert_eq!(html_error(b"x='y z' w"), b"w");
    assert_eq!(html_error(b"\"open"), b"");
    assert_eq!(html_error(b"  lead"), b"lead");
}

#[test]
fn stripslashes_unescapes_quotes() {
    assert_eq!(&*stripslashes(b"x=\\\"y\\\""), b"x=\"y\"");
    assert_eq!(&*stripslashes(b"\\\\\""), b"\\\"");
    assert!(matches!(stripslashes(b"plain"), Cow::Borrowed(_)));
}

const INPUTS: &[&[u8]] = &[
    b"",
    b"\x00",
    b"a\x00b",
    b"\\0",
    b"\\\\0",
    b"\\00\\000",
    b"a\\0b\\\\00c",
    b"\x01\x02\x08\t\n\x0B\x0C\r\x0E\x1F\x20\x7F",
    b"java\x00script:",
    b"\\\"",
    b"\\\\\"",
    b"\"x\" 'y' z",
    b"'unterminated",
    b"\"",
    b"a\xC3(b\x80",
    b"x\\0\x00\\",
];

#[test]
fn match_php() {
    let mut php = PhpOracle::start().unwrap();
    for input in INPUTS {
        let r =
            |php: &mut PhpOracle, f: &str, args| from_bytes(&php.call(f, args).unwrap().returned);
        let keep = json!({"array": [[{"b64": "c2xhc2hfemVybw=="}, {"b64": "a2VlcA=="}]]});
        assert_eq!(
            &*no_null(input, true),
            &r(&mut php, "wp_kses_no_null", vec![bytes(input), keep])[..],
            "{input:?}"
        );
        assert_eq!(
            &*no_null(input, false),
            &r(&mut php, "wp_kses_no_null", vec![bytes(input)])[..],
            "{input:?}"
        );
        assert_eq!(
            &*stripslashes(input),
            &r(&mut php, "wp_kses_stripslashes", vec![bytes(input)])[..],
            "{input:?}"
        );
        assert_eq!(
            html_error(input),
            &r(&mut php, "wp_kses_html_error", vec![bytes(input)])[..],
            "{input:?}"
        );
    }
}
