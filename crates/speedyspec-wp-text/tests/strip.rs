//! `strip_tags` and `wp_strip_all_tags`, checked against the corpus.

mod common;

use speedyspec_hooks::Value;
use speedyspec_wp_text::strings::to_php_string;
use speedyspec_wp_text::{strip_tags, wp_strip_all_tags_core};
use speedyspec_wp_text_corpus::{Php, assert_corpus, assert_corpus_reporting, to_value};

#[test]
fn strip_tags_matches_php() {
    assert_corpus("strip_tags", |args| Php::Str(strip_tags(common::text(&args[0])).into_owned()));
}

#[test]
fn strip_tags_matches_php_states() {
    assert_eq!(&*strip_tags(b"<a title='>'>x</a>"), b"x");
    assert_eq!(&*strip_tags(b"a<?php echo 1; ?>b"), b"ab");
    assert_eq!(&*strip_tags(b"a < b > c"), b"a < b > c");
    assert_eq!(&*strip_tags(b"a<!-- <b> -->c"), b"ac");
    assert!(matches!(strip_tags(b"plain"), std::borrow::Cow::Borrowed(_)));
}

#[test]
fn wp_strip_all_tags_matches_wordpress() {
    assert_corpus_reporting("wp_strip_all_tags", |args| {
        let remove_breaks = args.get(1).is_some_and(|b| !common::php_empty(b));
        match &args[0] {
            Php::Null => (Php::Str(Vec::new()), common::none()),
            value => match to_php_string(&to_value(value)) {
                Some(text) => (Php::Str(wp_strip_all_tags_core(&text, remove_breaks).into_owned()), common::none()),
                None => {
                    let kind = if matches!(to_value(value), Value::Array(_)) { "array" } else { "object" };
                    let message = format!(
                        "Warning: wp_strip_all_tags expects parameter #1 ($text) to be a string, {kind} given."
                    );
                    (Php::Str(Vec::new()), common::report(&["trigger_error", "", &message], Some(512)))
                }
            },
        }
    });
}
