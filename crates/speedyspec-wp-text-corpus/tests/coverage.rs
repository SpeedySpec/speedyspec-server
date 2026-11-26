//! Every function plans 1 and 2 port has its corpus covered by a test.

use speedyspec_wp_text_corpus::coverage;

/// What `speedyspec-wp-html-split` and `speedyspec-wp-text-escape` port, each checked by their crates' tests.
const ESCAPING: &[&str] = &[
    "wp_html_split",
    "htmlspecialchars",
    "_wp_specialchars",
    "wp_specialchars_decode",
    "wp_check_invalid_utf8",
    "esc_html",
    "esc_attr",
    "esc_textarea",
    "esc_xml",
    "esc_js",
    "tag_escape",
    "seems_utf8",
    "htmlentities2",
    "wp_slash",
    "wp_unslash",
    "addslashes_gpc",
    "backslashit",
];

#[test]
fn escaping_is_covered() {
    let report = coverage(ESCAPING);
    for function in ESCAPING {
        assert!(!report.skipped.contains_key(*function), "{function} has corpus cases but isn't ported");
        assert!(
            !speedyspec_wp_text_corpus::cases(function).is_empty(),
            "{function} has no corpus cases: the name is misspelt or the corpus lost them"
        );
    }
}
