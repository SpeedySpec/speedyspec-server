//! A WordPress release prepared for the native media functions: `wp-admin/includes/media.php`, `wp-includes/media.php`
//! and `wp-includes/functions.php` without the functions this port declares (which would otherwise be declared twice), every other file the release's own. Only WordPress's
//! own tests need it: the runtime never loads WordPress.

use std::path::{Path, PathBuf};

use speedyspec_compat::ReleaseMirror;

/// The functions the native area declares, by the release file that declares them.
pub const PORTED_FUNCTIONS: &[(&str, &str)] = &[
    ("wp-admin/includes/media.php", "wp_add_id3_tag_data"),
    ("wp-admin/includes/media.php", "wp_read_video_metadata"),
    ("wp-admin/includes/media.php", "wp_read_audio_metadata"),
    (
        "wp-admin/includes/media.php",
        "wp_get_media_creation_timestamp",
    ),
    ("wp-includes/media.php", "wp_getimagesize"),
    ("wp-includes/media.php", "wp_get_avif_info"),
    ("wp-includes/media.php", "wp_get_webp_info"),
    ("wp-includes/functions.php", "wp_get_image_mime"),
];

/// `file`'s text (a path under the release root) without the ported functions it declares, each with its docblock.
/// A function is a line `function <name>(` at the start of a line through the next line that is `}` alone, as
/// WordPress formats them.
pub fn strip_ported(file: &str, source: &str) -> Result<String, String> {
    let names: Vec<&str> = PORTED_FUNCTIONS
        .iter()
        .filter(|(f, _)| *f == file)
        .map(|(_, n)| *n)
        .collect();
    if names.is_empty() {
        return Err(format!("the media port strips no {file}"));
    }
    let lines: Vec<&str> = source.split_inclusive('\n').collect();
    let mut keep = vec![true; lines.len()];
    for name in names {
        let start = lines
            .iter()
            .position(|l| l.starts_with(&format!("function {name}(")))
            .ok_or_else(|| format!("{file} has no function {name}()"))?;
        let end = (start..lines.len())
            .find(|&i| lines[i].trim_end() == "}")
            .ok_or_else(|| format!("{file}'s {name}() has no end"))?;
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

/// Prepares `release` (a `wordpress/` root) in `out`: every entry is a link to the release's, except the stripped
/// files, written without the ported functions, and the directories holding them, made real so their other entries
/// can be links. Reuses a copy already prepared, rewriting a stripped file that differs. Returns `out`.
pub fn prepare_release(release: &Path, out: &Path) -> Result<PathBuf, String> {
    let mirror = ReleaseMirror::new(release, out)?;
    let mut stripped: Vec<&str> = PORTED_FUNCTIONS.iter().map(|(f, _)| *f).collect();
    stripped.dedup();
    for file in stripped {
        mirror.write(file, &strip_ported(file, &mirror.read(file)?)?)?;
    }
    Ok(out.to_path_buf())
}
