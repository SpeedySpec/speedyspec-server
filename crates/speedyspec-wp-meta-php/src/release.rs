//! A WordPress release prepared for the native meta functions: its `meta.php` without the functions this port
//! declares (which would otherwise be declared twice), its `class-wp-metadata-lazyloader.php` replaced by a file that
//! declares nothing (the area declares the class), every other file the release's own.

use std::path::{Path, PathBuf};

use speedyspec_compat::ReleaseMirror;

/// The `meta.php` functions the native area declares: all of WordPress 7.2-RC2's but `get_meta_sql()`, which stays
/// WordPress's own because it needs `WP_Meta_Query`.
pub const PORTED_FUNCTIONS: &[&str] = &[
    "add_metadata",
    "update_metadata",
    "delete_metadata",
    "get_metadata",
    "get_metadata_raw",
    "get_metadata_default",
    "metadata_exists",
    "get_metadata_by_mid",
    "update_metadata_by_mid",
    "delete_metadata_by_mid",
    "update_meta_cache",
    "wp_metadata_lazyloader",
    "_get_meta_table",
    "is_protected_meta",
    "sanitize_meta",
    "register_meta",
    "filter_default_metadata",
    "registered_meta_key_exists",
    "unregister_meta_key",
    "get_registered_meta_keys",
    "get_registered_metadata",
    "_wp_register_meta_args_allowed_list",
    "get_object_subtype",
];

/// The release file that declares `WP_Metadata_Lazyloader`, which the area declares instead.
pub const LAZYLOADER_FILE: &str = "wp-includes/class-wp-metadata-lazyloader.php";

/// What the prepared release holds at [`LAZYLOADER_FILE`]: `meta.php` still requires it.
pub const LAZYLOADER_STUB: &str = "<?php\n\
// WP_Metadata_Lazyloader is declared by SpeedySpec's native meta area (speedyspec-wp-meta-php's lazyloader.php)\n\
// at the start of every request, so this file declares nothing.\n";

/// `meta.php`'s text without the ported functions, each with its docblock. A function is a line
/// `function <name>(` at the start of a line through the next line that is `}` alone, as WordPress formats them.
pub fn strip_ported(source: &str) -> Result<String, String> {
    let lines: Vec<&str> = source.split_inclusive('\n').collect();
    let mut keep = vec![true; lines.len()];
    for name in PORTED_FUNCTIONS {
        let start = lines
            .iter()
            .position(|l| l.starts_with(&format!("function {name}(")))
            .ok_or_else(|| format!("meta.php has no function {name}()"))?;
        let end = (start..lines.len())
            .find(|&i| lines[i].trim_end() == "}")
            .ok_or_else(|| format!("meta.php's {name}() has no end"))?;
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

/// Prepares `release` (a `wordpress/` root) in `out`: links to each of its entries, except `wp-includes/meta.php`,
/// written without the ported functions, and [`LAZYLOADER_FILE`], written as [`LAZYLOADER_STUB`]. Reuses a copy
/// already prepared, rewriting both files when they differ. Returns `out`.
pub fn prepare_release(release: &Path, out: &Path) -> Result<PathBuf, String> {
    let mirror = ReleaseMirror::new(release, out)?;
    let meta = "wp-includes/meta.php";
    mirror.write(meta, &strip_ported(&mirror.read(meta)?)?)?;
    mirror.write(LAZYLOADER_FILE, LAZYLOADER_STUB)?;
    Ok(out.to_path_buf())
}
