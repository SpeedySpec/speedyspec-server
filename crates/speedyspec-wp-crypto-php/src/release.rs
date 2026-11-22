//! A WordPress release prepared for the native sodium functions: `wp-includes/functions.php` and
//! `wp-admin/includes/file.php` without the functions this port declares (which would otherwise be declared twice),
//! every other file the release's own. Only WordPress's own tests need it: the runtime never loads WordPress.

use std::path::{Path, PathBuf};

/// The functions the native area declares, by the release file that declares them.
pub const PORTED_FUNCTIONS: &[(&str, &str)] = &[
    ("wp-includes/functions.php", "wp_fast_hash"),
    ("wp-includes/functions.php", "wp_verify_fast_hash"),
    ("wp-admin/includes/file.php", "verify_file_signature"),
    ("wp-admin/includes/file.php", "wp_trusted_keys"),
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
        return Err(format!("the crypto port strips no {file}"));
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
        let target = out.join(file);
        if std::fs::read_to_string(&target).ok().as_deref() != Some(wanted.as_str()) {
            let staged = target.with_extension(format!("php.{}", std::process::id()));
            std::fs::write(&staged, wanted).map_err(|e| format!("{}: {e}", staged.display()))?;
            std::fs::rename(&staged, &target).map_err(|e| format!("{}: {e}", target.display()))?;
        }
    }
    Ok(out.to_path_buf())
}

/// Mirrors the directory `relative` of `release` into `out`: entries on the way to a stripped file are descended
/// into, stripped files are left for the caller, everything else is linked.
fn mirror(release: &Path, out: &Path, relative: &Path, stripped: &[&str]) -> Result<(), String> {
    let dir = release.join(relative);
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

fn link(from: &Path, to: &Path) -> Result<(), String> {
    if to.symlink_metadata().is_ok() {
        return Ok(());
    }
    std::os::unix::fs::symlink(from, to).map_err(|e| format!("{}: {e}", to.display()))
}
