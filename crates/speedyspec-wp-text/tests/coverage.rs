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
];

#[test]
fn content_filters_cover_their_corpus() {
    let implemented: Vec<&str> = CONTENT_FILTERS.iter().chain(EARLIER).copied().collect();
    let report = coverage(&implemented);
    for function in CONTENT_FILTERS {
        assert!(!report.skipped.contains_key(*function), "{function} has corpus cases no test covers");
    }
    // What is left waits for plan 2's last tasks (kses-lexical): the esc_* family, _wp_specialchars,
    // wp_check_invalid_utf8.
    let left: Vec<&String> = report.skipped.keys().collect();
    assert!(
        left.iter().all(|f| f.starts_with("esc_") || *f == "_wp_specialchars" || *f == "wp_check_invalid_utf8"),
        "unexpected uncovered functions: {left:?}"
    );
    assert!(report.covered > 900, "only {} cases covered", report.covered);
}
