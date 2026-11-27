//! A WordPress release prepared for the native post functions: its `post.php` without the functions this area
//! declares (which would otherwise be declared twice), every other file the release's own.
//!
//! [`prepare_release`] writes only `post.php` and keeps whatever another area already wrote in the same copy, so the
//! areas' preparations chain: a release that runs beside the native meta functions is prepared by
//! `speedyspec_wp_meta_php::prepare_release` and then by this one, into the same directory (the scenario runner's
//! caller does so). The chain is the caller's, not a dependency of this crate: the meta seam's PHP crate is another
//! seam's implementation.

use std::path::{Path, PathBuf};

use speedyspec_compat::ReleaseMirror;

/// The `post.php` functions the native area declares (WordPress 7.2-RC2): the post type and post status registries,
/// then post reading and the post caches. The meta wrappers (`get_post_meta()` and the rest), the functions that call
/// `WP_Query` (`get_posts()`, `get_children()`, `get_pages()`, `wp_get_recent_posts()`, …),
/// `update_post_author_caches()` and the attachment readers stay WordPress's own.
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
    "get_post",
    "get_post_ancestors",
    "get_post_field",
    "get_post_mime_type",
    "get_post_status",
    "get_post_type",
    "wp_get_post_parent_id",
    "get_page",
    "get_page_by_path",
    "get_page_children",
    "get_page_hierarchy",
    "_page_traverse_name",
    "get_page_uri",
    "get_all_page_ids",
    "_count_posts_cache_key",
    "wp_count_posts",
    "wp_count_attachments",
    "get_available_post_mime_types",
    "get_lastpostdate",
    "get_lastpostmodified",
    "_get_last_post_time",
    "update_post_cache",
    "clean_post_cache",
    "update_post_caches",
    "update_post_parent_caches",
    "update_postmeta_cache",
    "_prime_post_caches",
    "_prime_post_parent_id_caches",
    "wp_cache_set_posts_last_changed",
    "sanitize_post",
    "sanitize_post_field",
    "get_pung",
    "get_to_ping",
    "get_enclosed",
    "is_sticky",
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
/// written without the ported functions. Reuses a copy already prepared, keeping the files another area's
/// preparation wrote there (see the module's docs). Returns `out`.
pub fn prepare_release(release: &Path, out: &Path) -> Result<PathBuf, String> {
    let mirror = ReleaseMirror::new(release, out)?;
    let post = "wp-includes/post.php";
    mirror.write(post, &strip_ported(&mirror.read(post)?)?)?;
    Ok(out.to_path_buf())
}
