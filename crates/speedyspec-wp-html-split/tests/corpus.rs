//! `wp_html_split` returns what WordPress 7.1.5 returned for every recorded case, including every whole post.

use speedyspec_wp_html_split::wp_html_split;
use speedyspec_wp_text_corpus::{Php, assert_corpus};

#[test]
fn matches_wordpress() {
    assert_corpus("wp_html_split", |args| {
        Php::list(wp_html_split(args[0].bytes().expect("a string argument")))
    });
}
