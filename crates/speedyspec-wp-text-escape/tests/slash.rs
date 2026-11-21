//! Slashing, `backslashit()` and `tag_escape()` give WordPress's bytes.

mod support;

use speedyspec_wp_text_corpus::{Php, assert_corpus};
use speedyspec_wp_text_escape::{
    backslashit, stripslashes, tag_escape_core, wp_slash_value, wp_unslash_value,
};

#[test]
fn wp_slash_matches_wordpress() {
    assert_corpus("wp_slash", |args| {
        support::php(&wp_slash_value(&support::value(&args[0])))
    });
}

#[test]
fn wp_unslash_matches_wordpress() {
    assert_corpus("wp_unslash", |args| {
        support::php(&wp_unslash_value(&support::value(&args[0])))
    });
}

#[test]
fn backslashit_matches_wordpress() {
    assert_corpus("backslashit", |args| {
        Php::Str(backslashit(args[0].bytes().unwrap()))
    });
}

#[test]
fn tag_escape_matches_wordpress() {
    // The corpus registers no tag_escape filter, so the filtered result is the core's.
    assert_corpus("tag_escape", |args| {
        Php::Str(tag_escape_core(args[0].bytes().unwrap()).into_owned())
    });
}

#[test]
fn stripslashes_trailing_backslash() {
    assert_eq!(&*stripslashes(b"a\\"), b"a");
}
