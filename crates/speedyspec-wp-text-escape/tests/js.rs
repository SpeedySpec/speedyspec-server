//! The deprecated `seems_utf8()`, and the steps `esc_js()` takes after `_wp_specialchars()`.

use speedyspec_wp_text_corpus::{Php, PhpKey, assert_corpus_reporting};
use speedyspec_wp_text_escape::{esc_js_after_specialchars, seems_utf8_core};

fn list(items: Vec<Php>) -> Php {
    Php::Array(
        items
            .into_iter()
            .enumerate()
            .map(|(i, item)| (PhpKey::Int(i as i64), item))
            .collect(),
    )
}

fn text(s: &str) -> Php {
    Php::Str(s.as_bytes().to_vec())
}

#[test]
fn seems_utf8_matches_wordpress() {
    // WordPress reports the deprecation on every call; this is the recorder's shape of that report.
    let reported = list(vec![list(vec![
        text("deprecated_function"),
        text("seems_utf8"),
        text("6.9.0"),
        text("wp_is_valid_utf8()"),
    ])]);
    assert_corpus_reporting("seems_utf8", |args| {
        (
            Php::Bool(seems_utf8_core(args[0].bytes().unwrap())),
            reported.clone(),
        )
    });
}

#[test]
fn js_steps() {
    assert_eq!(
        esc_js_after_specialchars(b"a\\'b&#039;\r\nc"),
        b"a\\'b\\'\\nc".to_vec()
    );
}
