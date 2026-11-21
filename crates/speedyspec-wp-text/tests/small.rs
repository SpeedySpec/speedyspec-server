//! The small content filters, checked against the corpus.

use std::borrow::Cow;

use speedyspec_wp_text::{
    capital_p_dangit, convert_chars, convert_invalid_entities, ent2ncr, format_for_editor_core, normalize_whitespace,
};
use speedyspec_wp_text_corpus::{Php, PhpKey, assert_corpus, assert_corpus_reporting};
use speedyspec_wp_text_escape::Charset;

fn text(args: &[Php]) -> &[u8] {
    args[0].bytes().expect("a string argument")
}

fn php(bytes: Cow<'_, [u8]>) -> Php {
    Php::Str(bytes.into_owned())
}

/// PHP's `empty()` on a recorded argument.
fn php_empty(value: &Php) -> bool {
    match value {
        Php::Null | Php::Bool(false) => true,
        Php::Int(i) => *i == 0,
        Php::Float(f) => *f == 0.0,
        Php::Str(s) => s.is_empty() || s == b"0",
        Php::Array(a) => a.is_empty(),
        _ => false,
    }
}

#[test]
fn normalize_whitespace_matches_wordpress() {
    assert_corpus("normalize_whitespace", |args| php(normalize_whitespace(text(args))));
}

#[test]
fn convert_invalid_entities_matches_wordpress() {
    assert_corpus("convert_invalid_entities", |args| php(convert_invalid_entities(text(args))));
}

#[test]
fn ent2ncr_matches_wordpress() {
    assert_corpus("ent2ncr", |args| php(ent2ncr(text(args))));
}

#[test]
fn convert_chars_matches_wordpress() {
    assert_corpus_reporting("convert_chars", |args| {
        let reported = match args.get(1) {
            Some(deprecated) if !php_empty(deprecated) => Php::Array(vec![(
                PhpKey::Int(0),
                Php::Array(
                    ["deprecated_argument", "convert_chars", "0.71", ""]
                        .iter()
                        .enumerate()
                        .map(|(i, s)| (PhpKey::Int(i as i64), Php::Str(s.as_bytes().to_vec())))
                        .collect(),
                ),
            )]),
            _ => Php::Array(Vec::new()),
        };
        (php(convert_chars(text(args))), reported)
    });
}

#[test]
fn capital_p_dangit_matches_wordpress() {
    assert_corpus("capital_P_dangit", |args| php(capital_p_dangit(text(args), false, b"&#8220;")));
}

#[test]
fn capital_p_dangit_in_titles_matches_wordpress() {
    assert_corpus("speedyspec_capital_P_dangit_in_filter", |args| {
        let filter = args[1].bytes().unwrap();
        let in_title = filter == b"the_title" || filter == b"wp_title";
        php(capital_p_dangit(text(args), in_title, b"&#8220;"))
    });
}

#[test]
fn format_for_editor_matches_wordpress() {
    assert_corpus("format_for_editor", |args| php(format_for_editor_core(text(args), &Charset::from_php_name("UTF-8"))));
}

#[test]
fn borrows_when_unchanged() {
    let plain = b"plain";
    assert!(matches!(normalize_whitespace(plain), Cow::Borrowed(_)));
    assert!(matches!(convert_chars(plain), Cow::Borrowed(_)));
    assert!(matches!(convert_invalid_entities(plain), Cow::Borrowed(_)));
    assert!(matches!(ent2ncr(plain), Cow::Borrowed(_)));
    assert!(matches!(capital_p_dangit(plain, false, b"&#8220;"), Cow::Borrowed(_)));
    assert!(matches!(format_for_editor_core(plain, &Charset::from_php_name("UTF-8")), Cow::Borrowed(_)));
}
