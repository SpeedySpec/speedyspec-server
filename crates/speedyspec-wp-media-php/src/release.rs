//! A WordPress release prepared for the native media functions: `wp-admin/includes/media.php`, `wp-includes/media.php`
//! and `wp-includes/functions.php` without the functions this port declares (which would otherwise be declared twice), every other file the release's own. Only WordPress's
//! own tests need it: the runtime never loads WordPress.

use std::path::{Path, PathBuf};

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
    let mut stripped: Vec<&str> = PORTED_FUNCTIONS.iter().map(|(f, _)| *f).collect();
    stripped.dedup();
    mirror(release, out, Path::new(""), &stripped)?;
    for file in stripped {
        let source = release.join(file);
        let text =
            std::fs::read_to_string(&source).map_err(|e| format!("{}: {e}", source.display()))?;
        let wanted = strip_ported(file, &text)?;
        write_stripped(&out.join(file), &wanted)?;
    }
    Ok(out.to_path_buf())
}

/// Mirrors the directory `relative` of `release` into `out`: entries on the way to a stripped file are descended
/// into, stripped files are left for the caller, everything else is linked.
fn mirror(release: &Path, out: &Path, relative: &Path, stripped: &[&str]) -> Result<(), String> {
    let dir = release.join(relative);
    // A directory an earlier preparation linked whole must become real before anything is written under it:
    // writing through the link would change the release itself, which every compat suite shares.
    let target = out.join(relative);
    if target
        .symlink_metadata()
        .is_ok_and(|m| m.file_type().is_symlink())
    {
        std::fs::remove_file(&target).map_err(|e| format!("{}: {e}", target.display()))?;
    }
    std::fs::create_dir_all(out.join(relative)).map_err(|e| format!("{}: {e}", out.display()))?;
    for entry in std::fs::read_dir(&dir).map_err(|e| format!("{}: {e}", dir.display()))? {
        let entry = entry.map_err(|e| format!("{}: {e}", dir.display()))?;
        let path = relative.join(entry.file_name());
        let name = path.to_string_lossy();
        if stripped.contains(&name.as_ref()) {
            continue;
        }
        if stripped.iter().any(|f| f.starts_with(&format!("{name}/"))) {
            mirror(release, out, &path, stripped)?;
        } else {
            link(&entry.path(), &out.join(&path))?;
        }
    }
    Ok(())
}

/// Writes a stripped file at `target`, replacing a link an earlier preparation left there rather than writing through
/// it, and only when its text differs.
fn write_stripped(target: &Path, wanted: &str) -> Result<(), String> {
    let link = target
        .symlink_metadata()
        .is_ok_and(|m| m.file_type().is_symlink());
    if !link && std::fs::read_to_string(target).ok().as_deref() == Some(wanted) {
        return Ok(());
    }
    if link {
        std::fs::remove_file(target).map_err(|e| format!("{}: {e}", target.display()))?;
    }
    let staged = target.with_extension(format!("php.{}", std::process::id()));
    std::fs::write(&staged, wanted).map_err(|e| format!("{}: {e}", staged.display()))?;
    std::fs::rename(&staged, target).map_err(|e| format!("{}: {e}", target.display()))
}

fn link(from: &Path, to: &Path) -> Result<(), String> {
    if to.symlink_metadata().is_ok() {
        return Ok(());
    }
    std::os::unix::fs::symlink(from, to).map_err(|e| format!("{}: {e}", to.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tree prepared before a file under one of its linked directories became ported must not write through that
    /// link into the release (which every compat suite shares).
    #[test]
    fn a_linked_directory_is_replaced_before_writing_under_it() {
        let release = tempfile::tempdir().unwrap();
        let out = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(release.path().join("a")).unwrap();
        std::fs::write(release.path().join("a/f.php"), "original").unwrap();
        std::fs::write(release.path().join("a/g.php"), "other").unwrap();
        mirror(release.path(), out.path(), Path::new(""), &[]).unwrap();
        assert!(
            out.path()
                .join("a")
                .symlink_metadata()
                .unwrap()
                .file_type()
                .is_symlink()
        );

        mirror(release.path(), out.path(), Path::new(""), &["a/f.php"]).unwrap();
        assert!(
            out.path().join("a").symlink_metadata().unwrap().is_dir(),
            "a is a real directory now"
        );
        write_stripped(&out.path().join("a/f.php"), "stripped").unwrap();
        assert_eq!(
            std::fs::read_to_string(release.path().join("a/f.php")).unwrap(),
            "original"
        );
        assert_eq!(
            std::fs::read_to_string(out.path().join("a/g.php")).unwrap(),
            "other"
        );
    }
}
