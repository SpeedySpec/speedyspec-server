//! PHP's `htmlspecialchars()` gives the same bytes as the embedded PHP did when the corpus was recorded.

use std::borrow::Cow;

use speedyspec_wp_text_corpus::{Php, assert_corpus};
use speedyspec_wp_text_escape::{
    Charset, EntFlags, SingleByte, canonical_charset, htmlspecialchars,
};

#[test]
fn matches_php_htmlspecialchars() {
    assert_corpus("htmlspecialchars", |args| {
        // PHP's defaults for the arguments a case leaves out: ENT_QUOTES (and ENT_SUBSTITUTE, which only matters
        // for invalid UTF-8, which no such case has), UTF-8, and double encoding.
        let text = args[0].bytes().expect("text");
        let flags = match args.get(1) {
            None => 3,
            Some(Php::Int(flags)) => *flags,
            Some(other) => panic!("flags {other}"),
        };
        let charset = args
            .get(2)
            .map_or("UTF-8", |c| std::str::from_utf8(c.bytes().expect("charset")).unwrap());
        let double_encode = args.get(3).is_none_or(|d| matches!(d, Php::Bool(true)));
        Php::Str(
            htmlspecialchars(
                text,
                EntFlags::from_php(flags),
                &Charset::from_php_name(charset),
                double_encode,
            )
            .into_owned(),
        )
    });
}

#[test]
fn charset_names() {
    assert_eq!(
        Charset::from_php_name("iso8859-1"),
        Charset::SingleByte(SingleByte::Iso8859_1)
    );
    assert_eq!(Charset::from_php_name("x-klingon"), Charset::Utf8);
    assert_eq!(
        Charset::from_php_name("sjis"),
        Charset::Unsupported("sjis".into())
    );
    assert_eq!(Charset::from_php_name("UTF-8"), Charset::Utf8);
}

#[test]
fn canonical_charset_is_wordpress() {
    assert_eq!(canonical_charset("utf8"), "UTF-8");
    assert_eq!(canonical_charset("ISO8859-1"), "ISO-8859-1");
    assert_eq!(canonical_charset("KOI8-R"), "KOI8-R");
}

#[test]
fn clean_input_is_borrowed() {
    assert!(matches!(
        htmlspecialchars(b"plain text", EntFlags::from_php(3), &Charset::Utf8, false),
        Cow::Borrowed(_)
    ));
}
