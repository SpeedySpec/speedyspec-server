//! A WordPress release prepared for the native option functions: its `option.php` without the functions this port
//! declares (which would otherwise be declared twice), every other file the release's own.

use std::path::{Path, PathBuf};

use speedyspec_compat::ReleaseMirror;

/// The `option.php` functions the native area declares.
pub const PORTED_FUNCTIONS: &[&str] = &[
    "get_option",
    "wp_prime_option_caches",
    "wp_prime_option_caches_by_group",
    "get_options",
    "wp_set_option_autoload_values",
    "wp_set_options_autoload",
    "wp_set_option_autoload",
    "wp_protect_special_option",
    "form_option",
    "wp_load_alloptions",
    "update_option",
    "add_option",
    "delete_option",
    "wp_determine_option_autoload_value",
    "wp_filter_default_autoload_value_via_option_size",
    "wp_autoload_values_to_autoload",
    "get_transient",
    "set_transient",
    "delete_transient",
    "delete_expired_transients",
    "get_site_transient",
    "set_site_transient",
    "delete_site_transient",
    "get_site_option",
    "add_site_option",
    "update_site_option",
    "delete_site_option",
    "get_network_option",
    "add_network_option",
    "update_network_option",
    "delete_network_option",
    "wp_prime_site_option_caches",
    "wp_prime_network_option_caches",
    "wp_load_core_site_options",
    "register_setting",
    "unregister_setting",
    "get_registered_settings",
    "filter_default_option",
];

/// `option.php`'s text without the ported functions, each with its docblock. A function is a line
/// `function <name>(` at the start of a line through the next line that is `}` alone, as WordPress formats them.
pub fn strip_ported(source: &str) -> Result<String, String> {
    let lines: Vec<&str> = source.split_inclusive('\n').collect();
    let mut keep = vec![true; lines.len()];
    for name in PORTED_FUNCTIONS {
        let start = lines
            .iter()
            .position(|l| l.starts_with(&format!("function {name}(")))
            .ok_or_else(|| format!("option.php has no function {name}()"))?;
        let end = (start..lines.len())
            .find(|&i| lines[i].trim_end() == "}")
            .ok_or_else(|| format!("option.php's {name}() has no end"))?;
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

/// Prepares `release` (a `wordpress/` root) in `out`: links to each of its entries, except `wp-includes/option.php`,
/// which is written without the ported functions. Reuses a copy already prepared, rewriting its `option.php` when
/// it was stripped of other functions. Returns `out`.
pub fn prepare_release(release: &Path, out: &Path) -> Result<PathBuf, String> {
    let mirror = ReleaseMirror::new(release, out)?;
    let option = "wp-includes/option.php";
    mirror.write(option, &strip_ported(&mirror.read(option)?)?)?;
    Ok(out.to_path_buf())
}
