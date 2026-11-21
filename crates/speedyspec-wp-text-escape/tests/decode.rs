//! `wp_specialchars_decode()` decodes exactly what WordPress decodes, for every quote style.

mod support;

use std::borrow::Cow;

use speedyspec_wp_text_corpus::{Php, assert_corpus};
use speedyspec_wp_text_escape::{QuoteStyle, wp_specialchars_decode};

#[test]
fn matches_wordpress() {
    assert_corpus("wp_specialchars_decode", |args| {
        let style = args
            .get(1)
            .map_or(QuoteStyle::NoQuotes, |s| QuoteStyle::for_decode(&support::value(s)));
        Php::Str(wp_specialchars_decode(args[0].bytes().unwrap(), style).into_owned())
    });
}

#[test]
fn no_ampersand_is_borrowed() {
    assert!(matches!(
        wp_specialchars_decode(b"plain <text>", QuoteStyle::Quotes),
        Cow::Borrowed(_)
    ));
}
