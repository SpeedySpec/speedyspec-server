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
    "wp_parse_url",
    "speedyspec_wp_parse_str",
    "sanitize_key",
    "sanitize_html_class",
    "sanitize_locale_name",
    "sanitize_mime_type",
    "sanitize_hex_color",
    "sanitize_hex_color_no_hash",
    "maybe_hash_hex_color",
    "sanitize_sql_orderby",
    "zeroise",
    "trailingslashit",
    "untrailingslashit",
    "wp_basename",
    "wp_iso_descrambler",
    "url_shorten",
    "wp_make_link_relative",
    "_deep_replace",
    "utf8_uri_encode",
    "remove_accents",
    "sanitize_title_with_dashes",
    "sanitize_user",
    "sanitize_file_name",
    "sanitize_title",
    "sanitize_title_for_query",
    "speedyspec_antispambot_seeded",
    "speedyspec_smilies_search",
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

/// Plan 4's functions whose corpus cases are recorded but not yet checked; each task takes its own off this list,
/// and the last one removes it.
const PENDING: &[&str] = &[
    "convert_smilies",
    "esc_url",
    "format_to_edit",
    "is_email",
    "links_add_base_url",
    "links_add_target",
    "make_clickable",
    "rawurlencode_deep",
    "sanitize_email",
    "sanitize_text_field",
    "sanitize_textarea_field",
    "sanitize_trackback_urls",
    "sanitize_url",
    "speedyspec_make_clickable_in_comment_text",
    "stripslashes_deep",
    "stripslashes_from_strings_only",
    "urldecode_deep",
    "urlencode_deep",
    "wp_pre_kses_less_than",
    "wp_rel_nofollow",
    "wp_rel_ugc",
    "wp_targeted_link_rel",
];

#[test]
fn content_filters_cover_their_corpus() {
    let implemented: Vec<&str> = CONTENT_FILTERS.iter().chain(EARLIER).chain(PENDING).copied().collect();
    let report = coverage(&implemented);
    for function in CONTENT_FILTERS {
        assert!(!report.skipped.contains_key(*function), "{function} has corpus cases no test covers");
    }
    // With plan 2 finished, every recorded function is ported.
    assert!(report.skipped.is_empty(), "functions with no port: {:?}", report.skipped);
    assert!(report.covered > 900, "only {} cases covered", report.covered);
}
