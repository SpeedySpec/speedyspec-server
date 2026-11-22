//! `esc_xml`'s core, against the corpus (no filter is registered there).

use speedyspec_wp_text_corpus::{Php, assert_corpus};
use speedyspec_wp_text_escape::{check_invalid_utf8_for, esc_xml_after_check};

#[test]
fn matches_wordpress() {
    assert_corpus("esc_xml", |args| {
        let checked = check_invalid_utf8_for(args[0].bytes().unwrap(), true, false);
        Php::Str(esc_xml_after_check(&checked, "UTF-8"))
    });
}
