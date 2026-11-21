//! `htmlentities2()` gives WordPress's bytes.

use speedyspec_wp_text_corpus::{Php, assert_corpus};
use speedyspec_wp_text_escape::htmlentities2;

#[test]
fn matches_wordpress() {
    assert_corpus("htmlentities2", |args| {
        Php::Str(htmlentities2(args[0].bytes().unwrap()))
    });
}
