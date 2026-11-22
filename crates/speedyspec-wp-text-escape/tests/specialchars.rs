//! `_wp_specialchars` and `wp_check_invalid_utf8`, against the corpus.

mod support;

use speedyspec_wp_text_corpus::{Php, assert_corpus};
use speedyspec_wp_text_escape::{QuoteStyle, check_invalid_utf8_for, wp_specialchars};

fn truthy(value: Option<&Php>) -> bool {
    match value {
        None | Some(Php::Null) | Some(Php::Bool(false)) => false,
        Some(Php::Int(i)) => *i != 0,
        Some(Php::Str(s)) => !(s.is_empty() || s == b"0"),
        Some(_) => true,
    }
}

#[test]
fn matches_wordpress() {
    assert_corpus("_wp_specialchars", |args| {
        let style = args.get(1).map_or(QuoteStyle::NoQuotes, |s| QuoteStyle::for_specialchars(&support::value(s)));
        // A falsy charset means blog_charset, which the corpus recorded as UTF-8.
        let charset = match args.get(2) {
            Some(Php::Str(s)) if !s.is_empty() && s != b"0" => String::from_utf8_lossy(s).into_owned(),
            _ => "UTF-8".to_string(),
        };
        let text = args[0].bytes().expect("a string");
        Php::Str(wp_specialchars(text, style, &charset, truthy(args.get(3))).into_owned())
    });
}

#[test]
fn wp_check_invalid_utf8_matches() {
    assert_corpus("wp_check_invalid_utf8", |args| {
        Php::Str(check_invalid_utf8_for(args[0].bytes().unwrap(), true, truthy(args.get(1))).into_owned())
    });
}
