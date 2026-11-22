//! How much of the formatting corpus the ported functions answer.

use speedyspec_wp_text_corpus::coverage;

/// Every function this crate's corpus tests check.
const CONTENT_FILTERS: &[&str] = &[
    "wptexturize",
    "speedyspec_wptexturize_translated",
    "speedyspec_wptexturize_no_texturize_shortcodes",
    "wpautop",
    "shortcode_unautop",
    "wp_replace_in_html_tags",
    "force_balance_tags",
    "balanceTags",
    "normalize_whitespace",
    "convert_chars",
    "convert_invalid_entities",
    "ent2ncr",
    "capital_P_dangit",
    "speedyspec_capital_P_dangit_in_filter",
    "strip_tags",
    "wp_strip_all_tags",
    "wp_trim_words",
    "speedyspec_wp_trim_words_characters",
    "wp_html_excerpt",
    "format_for_editor",
];

/// What plans 1 and 2 port (html-split and text-escape), which their own crates check.
const EARLIER: &[&str] = &[
    "wp_html_split",
    "htmlspecialchars",
    "wp_specialchars_decode",
    "wp_slash",
    "wp_unslash",
    "addslashes_gpc",
    "backslashit",
    "tag_escape",
    "seems_utf8",
    "htmlentities2",
    "_wp_specialchars",
    "wp_check_invalid_utf8",
    "esc_html",
    "esc_attr",
    "esc_textarea",
    "esc_xml",
    "esc_js",
];

#[test]
fn content_filters_cover_their_corpus() {
    let implemented: Vec<&str> = CONTENT_FILTERS.iter().chain(EARLIER).copied().collect();
    let report = coverage(&implemented);
    for function in CONTENT_FILTERS {
        assert!(!report.skipped.contains_key(*function), "{function} has corpus cases no test covers");
    }
    // With plan 2 finished, every recorded function is ported.
    assert!(report.skipped.is_empty(), "functions with no port: {:?}", report.skipped);
    assert!(report.covered > 900, "only {} cases covered", report.covered);
}
