//! `force_balance_tags`, checked against the corpus.

mod common;

use speedyspec_wp_text::force_balance_tags;
use speedyspec_wp_text_corpus::{Php, assert_corpus};

#[test]
fn force_balance_tags_matches_wordpress() {
    assert_corpus("force_balance_tags", |args| Php::Str(force_balance_tags(common::text(&args[0])).into_owned()));
}

#[test]
fn forced_balance_tags_matches_wordpress() {
    // balanceTags( $text, true ) is force_balance_tags( $text ); every recorded call forces.
    assert_corpus("balanceTags", |args| {
        assert!(!common::php_empty(&args[1]), "a recorded balanceTags call that doesn't force");
        Php::Str(force_balance_tags(common::text(&args[0])).into_owned())
    });
}

#[test]
fn balance_tags_closes_what_is_open() {
    assert_eq!(&*force_balance_tags(b"<b>x"), b"<b>x</b>");
}
