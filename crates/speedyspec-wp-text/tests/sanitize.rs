//! The character sanitizers, against the recorded corpus (no filters registered) and their patterns.

mod common;

use common::text;
use proptest::prelude::*;
use speedyspec_wp_text::preg::{Pattern, preg_replace};
use speedyspec_wp_text::sanitize::{
    maybe_hash_hex_color, sanitize_hex_color, sanitize_hex_color_no_hash, sanitize_html_class_core, sanitize_key_core,
    sanitize_locale_name_core, sanitize_mime_type_core, sanitize_sql_orderby,
};
use speedyspec_wp_text::strings::to_php_string;
use speedyspec_wp_text_corpus::{Php, assert_corpus, to_value};

/// A recorded scalar as PHP's `(string)` makes it, or `None` for an array.
fn scalar(value: &Php) -> Option<Vec<u8>> {
    to_php_string(&to_value(value))
}

#[test]
fn sanitize_key_corpus() {
    assert_corpus("sanitize_key", |args| {
        Php::Str(scalar(&args[0]).map_or_else(Vec::new, |key| sanitize_key_core(&key).into_owned()))
    });
}

#[test]
fn sanitize_html_class_corpus() {
    assert_corpus("sanitize_html_class", |args| {
        let class = sanitize_html_class_core(text(&args[0]));
        let fallback = args.get(1).map_or(&b""[..], text);
        let truthy = !(fallback.is_empty() || fallback == b"0");
        Php::Str(if class.is_empty() && truthy { sanitize_html_class_core(fallback).into_owned() } else { class.into_owned() })
    });
}

#[test]
fn sanitize_locale_name_corpus() {
    assert_corpus("sanitize_locale_name", |args| Php::Str(sanitize_locale_name_core(text(&args[0])).into_owned()));
}

#[test]
fn sanitize_mime_type_corpus() {
    assert_corpus("sanitize_mime_type", |args| Php::Str(sanitize_mime_type_core(text(&args[0])).into_owned()));
}

/// `''`, `null` or the string, as the hex-colour functions return.
fn optional(value: Option<&[u8]>) -> Php {
    value.map_or(Php::Null, |v| Php::Str(v.to_vec()))
}

#[test]
fn hex_color_corpora() {
    assert_corpus("sanitize_hex_color", |args| optional(sanitize_hex_color(text(&args[0]))));
    assert_corpus("sanitize_hex_color_no_hash", |args| optional(sanitize_hex_color_no_hash(text(&args[0]))));
    assert_corpus("maybe_hash_hex_color", |args| Php::Str(maybe_hash_hex_color(text(&args[0])).into_owned()));
}

#[test]
fn sanitize_sql_orderby_corpus() {
    assert_corpus("sanitize_sql_orderby", |args| {
        sanitize_sql_orderby(text(&args[0])).map_or(Php::Bool(false), |v| Php::Str(v.to_vec()))
    });
}

#[test]
fn hex_color_dollar_allows_a_final_newline() {
    // PCRE's `$` matches before a final newline, so WordPress keeps it.
    assert_eq!(sanitize_hex_color(b"#abc\n"), Some(&b"#abc\n"[..]));
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(4096))]

    #[test]
    fn class_sanitizers_match_patterns(input in proptest::collection::vec(any::<u8>(), 0..24)) {
        let lower = input.to_ascii_lowercase();
        let key = preg_replace(&Pattern::php("/[^a-z0-9_\\-]/"), &lower, b"", None).unwrap().0;
        prop_assert_eq!(&*sanitize_key_core(&input), &*key);
        let unescaped = preg_replace(&Pattern::php("|%[a-fA-F0-9][a-fA-F0-9]|"), &input, b"", None).unwrap().0.into_owned();
        let html = preg_replace(&Pattern::php("/[^A-Za-z0-9_-]/"), &unescaped, b"", None).unwrap().0;
        prop_assert_eq!(&*sanitize_html_class_core(&input), &*html);
        let locale = preg_replace(&Pattern::php("/[^A-Za-z0-9_-]/"), &input, b"", None).unwrap().0;
        prop_assert_eq!(&*sanitize_locale_name_core(&input), &*locale);
        let mime = preg_replace(&Pattern::php("/[^-+*.a-zA-Z0-9\\/]/"), &input, b"", None).unwrap().0;
        prop_assert_eq!(&*sanitize_mime_type_core(&input), &*mime);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(4096))]

    #[test]
    fn text_field_passes_match_patterns(input in proptest::collection::vec(proptest::sample::select(vec![
        b' ', b'\t', b'\n', b'\r', b'%', b'a', b'F', b'g', b'0', b'9', b'\x0B',
    ]), 0..24)) {
        use speedyspec_wp_text::sanitize::{collapse_runs, first_octet};
        let ws = preg_replace(&Pattern::php("/[\\r\\n\\t ]+/"), &input, b" ", None).unwrap().0.into_owned();
        prop_assert_eq!(collapse_runs(&input, |b| matches!(b, b'\r' | b'\n' | b'\t' | b' ')), ws);
        let spaces = preg_replace(&Pattern::php("/ +/"), &input, b" ", None).unwrap().0.into_owned();
        prop_assert_eq!(collapse_runs(&input, |b| b == b' '), spaces);
        let found = speedyspec_wp_text::preg::preg_match(&Pattern::php("/%[a-f0-9]{2}/i"), &input).unwrap().map(|m| m.get(0).unwrap().to_vec());
        prop_assert_eq!(first_octet(&input).map(|o| o.to_vec()), found);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(4096))]

    #[test]
    fn file_name_runs_match_patterns(input in proptest::collection::vec(proptest::sample::select(vec![
        b'.', b'-', b' ', b'\t', b'\n', b'\r', b'a', b'_', b'\x0B',
    ]), 0..24)) {
        use speedyspec_wp_text::sanitize::collapse_runs_to;
        let periods = preg_replace(&Pattern::php("/\\.{2,}/"), &input, b".", None).unwrap().0.into_owned();
        prop_assert_eq!(collapse_runs_to(&input, |b| b == b'.', b'.'), periods);
        let dashes = preg_replace(&Pattern::php("/[\\r\\n\\t -]+/"), &input, b"-", None).unwrap().0.into_owned();
        prop_assert_eq!(collapse_runs_to(&input, |b| matches!(b, b'\r' | b'\n' | b'\t' | b' ' | b'-'), b'-'), dashes);
    }
}
