//! The small formatting helpers, against the recorded corpus.

mod common;

use common::{php_int, text};
use speedyspec_wp_text::helpers::{
    antispambot_core, deep_replace, trailingslashit, untrailingslashit, url_shorten, utf8_uri_encode, wp_basename,
    wp_iso_descrambler, wp_make_link_relative, wp_truncate_slug, zeroise,
};
use speedyspec_wp_text::strings::to_php_string;
use speedyspec_wp_text_corpus::{Php, assert_corpus, to_value};

/// PHP's `(string)` of a recorded scalar.
fn string(value: &Php) -> Vec<u8> {
    to_php_string(&to_value(value)).expect("a scalar")
}

#[test]
fn zeroise_corpus() {
    assert_corpus("zeroise", |args| Php::Str(zeroise(&string(&args[0]), php_int(&args[1]))));
}

#[test]
fn zeroise_negative_threshold_pads_right() {
    assert_eq!(zeroise(b"a", -3), b"a00");
}

#[test]
fn slashit_corpora() {
    assert_corpus("trailingslashit", |args| Php::Str(trailingslashit(text(&args[0]))));
    assert_corpus("untrailingslashit", |args| Php::Str(untrailingslashit(text(&args[0])).to_vec()));
}

#[test]
fn wp_basename_corpus() {
    assert_corpus("wp_basename", |args| Php::Str(wp_basename(text(&args[0]), args.get(1).map_or(&b""[..], text))));
}

#[test]
fn wp_iso_descrambler_corpus() {
    assert_corpus("wp_iso_descrambler", |args| Php::Str(wp_iso_descrambler(text(&args[0])).into_owned()));
}

#[test]
fn url_shorten_corpus() {
    assert_corpus("url_shorten", |args| {
        Php::Str(url_shorten(text(&args[0]), args.get(1).map_or(35, php_int)))
    });
}

#[test]
fn wp_make_link_relative_corpus() {
    assert_corpus("wp_make_link_relative", |args| Php::Str(wp_make_link_relative(text(&args[0])).into_owned()));
}

#[test]
fn deep_replace_corpus() {
    assert_corpus("_deep_replace", |args| {
        let search: Vec<Vec<u8>> = match &args[0] {
            Php::Array(items) => items.iter().map(|(_, v)| string(v)).collect(),
            scalar => vec![string(scalar)],
        };
        let search: Vec<&[u8]> = search.iter().map(Vec::as_slice).collect();
        Php::Str(deep_replace(&search, &string(&args[1])))
    });
}

#[test]
fn utf8_uri_encode_corpus() {
    assert_corpus("utf8_uri_encode", |args| {
        let length = args.get(1).map_or(0, php_int).max(0) as usize;
        let ascii = args.get(2).is_some_and(|v| matches!(v, Php::Bool(true)));
        Php::Str(utf8_uri_encode(text(&args[0]), length, ascii))
    });
}

#[test]
fn utf8_uri_encode_cuts_whole_characters() {
    assert_eq!(utf8_uri_encode("aé€".as_bytes(), 5, false), b"a");
    assert_eq!(utf8_uri_encode("aé€".as_bytes(), 7, false), b"a%c3%a9");
}

#[test]
fn wp_truncate_slug_corpus() {
    assert_corpus("wp_truncate_slug", |args| {
        let length = args.get(1).map_or(200, php_int).max(0) as usize;
        Php::Str(wp_truncate_slug(text(&args[0]), length))
    });
}

#[test]
fn wp_truncate_slug_trims_dashes_even_when_short() {
    assert_eq!(wp_truncate_slug(b"abc-", 200), b"abc");
    assert_eq!(wp_truncate_slug(b"abc---", 200), b"abc");
    assert_eq!(wp_truncate_slug(b"a-b", 2), b"a");
    assert_eq!(wp_truncate_slug(b"---", 200), b"");
}

#[test]
fn wp_truncate_slug_plus_reencodes() {
    // `urldecode()` turns `+` into a space, so the slug is re-encoded: ASCII is `rawurlencode`d (uppercase hex),
    // multibyte characters keep lowercase hex, and a cut never splits a character.
    let slug = [&[b'a'; 99][..], b"+", &[b'b'; 99][..]].concat();
    let expected = [&[b'a'; 99][..], b"%20", &[b'b'; 96][..]].concat();
    assert_eq!(wp_truncate_slug(&slug, 198), expected);
    // Hex case is the encoder's, not the input's.
    assert_eq!(wp_truncate_slug(b"%2f%E2%82%AC%2f", 14), b"%2F%e2%82%ac");
    // A character that doesn't fit is dropped whole.
    assert_eq!(wp_truncate_slug(b"a+%E2%82%AC/", 11), b"a%20");
    // Not longer than the limit: kept as it is, `+` and all.
    assert_eq!(wp_truncate_slug(b"a+%E2%82%AC/", 12), b"a+%E2%82%AC/");
    assert_eq!(wp_truncate_slug(b"abcdef%e2%82%ac", 10), b"abcdef");
}

#[test]
fn antispambot_branches() {
    let mut choices = [0, 1, 2, 1, 1].into_iter();
    let mut rand = |_min: i64, _max: i64| choices.next().expect("a choice");
    assert_eq!(antispambot_core("é@x".as_bytes(), 1, &mut rand), b"&#233;&#64;%78");
    let mut ones = |_min: i64, _max: i64| 1;
    assert_eq!(antispambot_core(b"a\xFFb", 0, &mut ones), b"a\xFFb");
}
