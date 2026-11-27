//! A WordPress release prepared for the native registry functions: its `post.php` without the functions this area
//! declares (which would otherwise be declared twice), every other file the release's own.
//!
//! Task 8 of the reading-posts plan extends [`PORTED_FUNCTIONS`] with the reading functions and chains
//! [`prepare_release`] on meta-php's, so one prepared release serves both areas.

use std::path::{Path, PathBuf};

use speedyspec_compat::ReleaseMirror;

/// The `post.php` functions the native area declares (WordPress 7.2-RC2's post type and post status registries).
pub const PORTED_FUNCTIONS: &[&str] = &[
    "create_initial_post_types",
    "register_post_status",
    "get_post_status_object",
    "get_post_stati",
    "is_post_type_hierarchical",
    "post_type_exists",
    "get_post_type_object",
    "get_post_types",
    "register_post_type",
    "unregister_post_type",
    "get_post_type_capabilities",
    "get_post_type_labels",
    "_get_custom_object_labels",
    "add_post_type_support",
    "remove_post_type_support",
    "get_all_post_type_supports",
    "post_type_supports",
    "get_post_types_by_support",
    "is_post_type_viewable",
    "is_post_status_viewable",
];

/// `post.php`'s text without the ported functions, each with its docblock. A function is a line
/// `function <name>(` at the start of a line through the next line that is `}` alone, as WordPress formats them.
pub fn strip_ported(source: &str) -> Result<String, String> {
    let lines: Vec<&str> = source.split_inclusive('\n').collect();
    let mut keep = vec![true; lines.len()];
    for name in PORTED_FUNCTIONS {
        let start = lines
            .iter()
            .position(|l| l.starts_with(&format!("function {name}(")))
            .ok_or_else(|| format!("post.php has no function {name}()"))?;
        let end = (start..lines.len())
            .find(|&i| lines[i].trim_end() == "}")
            .ok_or_else(|| format!("post.php's {name}() has no end"))?;
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

/// Prepares `release` (a `wordpress/` root) in `out`: links to each of its entries, except `wp-includes/post.php`,
/// written without the ported functions. Reuses a copy already prepared. Returns `out`.
pub fn prepare_release(release: &Path, out: &Path) -> Result<PathBuf, String> {
    let mirror = ReleaseMirror::new(release, out)?;
    let post = "wp-includes/post.php";
    mirror.write(post, &strip_ported(&mirror.read(post)?)?)?;
    Ok(out.to_path_buf())
}
