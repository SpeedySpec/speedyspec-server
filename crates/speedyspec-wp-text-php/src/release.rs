//! The release step: WordPress's `formatting.php` without the functions this crate provides natively.

use std::path::{Path, PathBuf};

use speedyspec_compat::ReleaseMirror;

/// Every function of `wp-includes/formatting.php` this crate provides natively (the names `native_names()` returns).
pub const PORTED_FUNCTIONS: &[&str] = &[
    "wp_html_split",
    "get_html_split_regex",
    "_wp_specialchars",
    "wp_specialchars_decode",
    "wp_check_invalid_utf8",
    "seems_utf8",
    "esc_html",
    "esc_attr",
    "esc_textarea",
    "esc_xml",
    "esc_js",
    "htmlentities2",
    "wp_slash",
    "wp_unslash",
    "backslashit",
    "tag_escape",
    "wptexturize",
    "wpautop",
    "wp_replace_in_html_tags",
    "shortcode_unautop",
    "convert_chars",
    "convert_invalid_entities",
    "ent2ncr",
    "balanceTags",
    "force_balance_tags",
    "capital_P_dangit",
    "wp_trim_words",
    "wp_html_excerpt",
    "normalize_whitespace",
    "wp_strip_all_tags",
    "format_for_editor",
    "wp_spaces_regexp",
    "convert_smilies",
    "translate_smiley",
    "make_clickable",
    "_split_str_by_whitespace",
    "wp_rel_callback",
    "wp_rel_nofollow",
    "wp_rel_nofollow_callback",
    "wp_rel_ugc",
    "wp_targeted_link_rel",
    "wp_targeted_link_rel_callback",
    "links_add_base_url",
    "links_add_target",
    "format_to_edit",
    "wp_pre_kses_less_than",
    "sanitize_title",
    "sanitize_title_for_query",
    "sanitize_title_with_dashes",
    "remove_accents",
    "utf8_uri_encode",
    "wp_truncate_slug",
    "sanitize_file_name",
    "sanitize_user",
    "sanitize_key",
    "sanitize_html_class",
    "sanitize_locale_name",
    "sanitize_text_field",
    "sanitize_textarea_field",
    "sanitize_email",
    "is_email",
    "sanitize_hex_color",
    "sanitize_hex_color_no_hash",
    "maybe_hash_hex_color",
    "sanitize_mime_type",
    "sanitize_trackback_urls",
    "sanitize_sql_orderby",
    "esc_url",
    "esc_url_raw",
    "sanitize_url",
    "_deep_replace",
    "wp_make_link_relative",
    "url_shorten",
    "antispambot",
    "zeroise",
    "trailingslashit",
    "untrailingslashit",
    "wp_basename",
    "wp_iso_descrambler",
    "wp_parse_str",
];

/// `formatting.php`'s text without [`PORTED_FUNCTIONS`], each with its docblock. A function is a line
/// `function <name>(` at the start of a line through the next line that is `}` alone, as WordPress formats them.
///
/// @param source
///     The release's `wp-includes/formatting.php`.
/// @return
///     The stripped text, or an error naming a listed function the file lacks.
pub fn strip_ported(source: &str) -> Result<String, String> {
    let lines: Vec<&str> = source.split_inclusive('\n').collect();
    let mut keep = vec![true; lines.len()];
    for name in PORTED_FUNCTIONS {
        let start = lines
            .iter()
            .position(|l| l.starts_with(&format!("function {name}(")))
            .ok_or_else(|| format!("formatting.php has no function {name}()"))?;
        let end = (start..lines.len())
            .find(|&i| lines[i].trim_end() == "}")
            .ok_or_else(|| format!("formatting.php's {name}() has no end"))?;
        let mut first = start;
        if first > 0 && lines[first - 1].trim_end() == " */" {
            while first > 0 && !lines[first - 1].starts_with("/**") {
                first -= 1;
            }
            first = first.saturating_sub(1);
        }
        keep[first..=end].iter_mut().for_each(|k| *k = false);
    }
    Ok(lines.iter().zip(keep).filter(|(_, k)| *k).map(|(l, _)| *l).collect())
}

/// Prepares `release` (a `wordpress/` root) in `out` for the native formatting functions: kses's prepared release
/// (`speedyspec_wp_kses_php::prepare_release`), with `wp-includes/formatting.php` written without
/// [`PORTED_FUNCTIONS`]. Reuses a copy already prepared, rewriting the stripped file when it differs.
///
/// @param release
///     The pinned release's `wordpress/` directory.
/// @param out
///     Where to prepare it.
/// @return
///     `out`.
pub fn prepare_release(release: &Path, out: &Path) -> Result<PathBuf, String> {
    speedyspec_wp_kses_php::prepare_release(release, out)?;
    let mirror = ReleaseMirror::new(release, out)?;
    let formatting = "wp-includes/formatting.php";
    mirror.write(formatting, &strip_ported(&mirror.read(formatting)?)?)?;
    Ok(out.to_path_buf())
}
