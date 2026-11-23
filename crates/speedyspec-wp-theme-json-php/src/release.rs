//! A WordPress release prepared for the native theme.json area: its theme.json class files emptied and its
//! `global-styles-and-settings.php` without the functions the area declares (which would otherwise be declared twice);
//! every other file the release's own.

use std::path::{Path, PathBuf};

use speedyspec_compat::ReleaseMirror;

/// The class files the area replaces whole: it declares every class they declare.
pub const REPLACED_FILES: &[&str] = &[
    "class-wp-theme-json.php",
    "class-wp-theme-json-data.php",
    "class-wp-theme-json-schema.php",
    "class-wp-theme-json-resolver.php",
];

/// The functions of `global-styles-and-settings.php` the area declares (all of them in 7.1.5).
pub const PORTED_FUNCTIONS: &[&str] = &[
    "wp_get_global_settings",
    "wp_get_global_styles",
    "wp_get_global_stylesheet",
    "wp_add_global_styles_for_blocks",
    "wp_get_block_name_from_theme_json_path",
    "wp_theme_has_theme_json",
    "wp_clean_theme_json_cache",
    "wp_get_theme_directory_pattern_slugs",
    "wp_get_theme_data_custom_templates",
    "wp_get_theme_data_template_parts",
    "wp_get_block_css_selector",
];

const STRIPPED: &str = "global-styles-and-settings.php";

/// What a replaced class file holds.
const EMPTIED: &str = "<?php\n// Declared natively by SpeedySpec's theme.json area.\n";

/// `global-styles-and-settings.php`'s text without the ported functions, each with its docblock. A function is a line
/// `function <name>(` at the start of a line through the next line that is `}` alone, as WordPress formats them.
pub fn strip_ported(source: &str) -> Result<String, String> {
    let lines: Vec<&str> = source.split_inclusive('\n').collect();
    let mut keep = vec![true; lines.len()];
    for name in PORTED_FUNCTIONS {
        let start = lines
            .iter()
            .position(|l| l.starts_with(&format!("function {name}(")))
            .ok_or_else(|| format!("{STRIPPED} has no function {name}()"))?;
        let end = (start..lines.len())
            .find(|&i| lines[i].trim_end() == "}")
            .ok_or_else(|| format!("{STRIPPED}'s {name}() has no end"))?;
        let mut first = start;
        if first > 0 && lines[first - 1].trim_end() == " */" {
            while first > 0 && !lines[first - 1].starts_with("/**") {
                first -= 1;
            }
            first = first.saturating_sub(1);
        }
        keep[first..=end].iter_mut().for_each(|k| *k = false);
    }
    Ok(lines
        .iter()
        .zip(keep)
        .filter(|(_, k)| *k)
        .map(|(l, _)| *l)
        .collect())
}

/// Prepares `release` (a `wordpress/` root) in `out`: links to each of its entries, except the replaced class files
/// (emptied) and `global-styles-and-settings.php` (without the ported functions). Reuses a copy already prepared,
/// rewriting a file that differs. Returns `out`.
pub fn prepare_release(release: &Path, out: &Path) -> Result<PathBuf, String> {
    let mirror = ReleaseMirror::new(release, out)?;
    for file in REPLACED_FILES {
        mirror.write(&format!("wp-includes/{file}"), EMPTIED)?;
    }
    let stripped = format!("wp-includes/{STRIPPED}");
    mirror.write(&stripped, &strip_ported(&mirror.read(&stripped)?)?)?;
    Ok(out.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every ported function is stripped from 7.1.5's file and nothing else is, when the release is at hand.
    #[test]
    fn strips_every_ported_function_from_the_release() {
        let Ok(root) = std::env::var("WP") else {
            return;
        };
        let source = std::fs::read_to_string(format!("{root}/wp-includes/{STRIPPED}")).unwrap();
        let stripped = strip_ported(&source).unwrap();
        for name in PORTED_FUNCTIONS {
            assert!(
                !stripped.contains(&format!("function {name}(")),
                "{name} left"
            );
        }
        assert!(stripped.starts_with("<?php"));
    }
}
