//! A WordPress release prepared for the native option functions: its `option.php` without the functions this port
//! declares (which would otherwise be declared twice), every other file the release's own.

use std::path::{Path, PathBuf};

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
/// which is written without the ported functions. Reuses a copy already prepared. Returns `out`.
pub fn prepare_release(release: &Path, out: &Path) -> Result<PathBuf, String> {
    let err = |p: &Path, e: std::io::Error| format!("{}: {e}", p.display());
    let option = out.join("wp-includes/option.php");
    if option.exists() {
        return Ok(out.to_path_buf());
    }
    std::fs::create_dir_all(out.join("wp-includes")).map_err(|e| err(out, e))?;
    for entry in std::fs::read_dir(release).map_err(|e| err(release, e))? {
        let entry = entry.map_err(|e| err(release, e))?;
        if entry.file_name() == "wp-includes" {
            continue;
        }
        link(&entry.path(), &out.join(entry.file_name()))?;
    }
    let includes = release.join("wp-includes");
    for entry in std::fs::read_dir(&includes).map_err(|e| err(&includes, e))? {
        let entry = entry.map_err(|e| err(&includes, e))?;
        if entry.file_name() == "option.php" {
            continue;
        }
        link(
            &entry.path(),
            &out.join("wp-includes").join(entry.file_name()),
        )?;
    }
    let source =
        std::fs::read_to_string(includes.join("option.php")).map_err(|e| err(&includes, e))?;
    let staged = out.join("wp-includes/option.php.partial");
    std::fs::write(&staged, strip_ported(&source)?).map_err(|e| err(&staged, e))?;
    std::fs::rename(&staged, &option).map_err(|e| err(&option, e))?;
    Ok(out.to_path_buf())
}

fn link(from: &Path, to: &Path) -> Result<(), String> {
    if to.exists() {
        return Ok(());
    }
    #[cfg(unix)]
    let made = std::os::unix::fs::symlink(from, to);
    #[cfg(not(unix))]
    let made = if from.is_dir() {
        Err(std::io::Error::other("links need unix"))
    } else {
        std::fs::copy(from, to).map(|_| ())
    };
    made.map_err(|e| format!("{}: {e}", to.display()))
}
