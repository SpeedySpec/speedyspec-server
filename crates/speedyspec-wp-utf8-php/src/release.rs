//! A WordPress release prepared for the native UTF-8 functions: `utf8.php` and `compat-utf8.php` replaced by stubs
//! (every function in them is declared natively), and `compat.php` without the three UTF-8 helpers this port
//! declares. Every other file is the release's own.

use std::path::{Path, PathBuf};

/// The `compat.php` functions the native area declares.
pub const PORTED_FUNCTIONS: &[&str] = &["_is_utf8_charset", "_mb_substr", "_mb_strlen"];

/// The files replaced whole, because the native area declares all their functions.
const STUBBED: &[&str] = &["utf8.php", "compat-utf8.php"];

const STUB: &str = "<?php\n// Provided by speedyspec-wp-utf8-php.\n";

/// `compat.php`'s text without the ported functions, each with its docblock. A function is a line
/// `function <name>(` at the start of a line through the next line that is `}` alone, as WordPress formats them.
pub fn strip_ported(source: &str) -> Result<String, String> {
    let lines: Vec<&str> = source.split_inclusive('\n').collect();
    let mut keep = vec![true; lines.len()];
    for name in PORTED_FUNCTIONS {
        let start = lines
            .iter()
            .position(|l| l.starts_with(&format!("function {name}(")))
            .ok_or_else(|| format!("compat.php has no function {name}()"))?;
        let end = (start..lines.len())
            .find(|&i| lines[i].trim_end() == "}")
            .ok_or_else(|| format!("compat.php's {name}() has no end"))?;
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

/// Prepares `release` (a `wordpress/` root) in `out`: links to each of its entries, except the stubbed files and
/// `compat.php`, which are written. Rewrites them in a copy already prepared, so it always matches this build.
/// Returns `out`.
pub fn prepare_release(release: &Path, out: &Path) -> Result<PathBuf, String> {
    let err = |p: &Path, e: std::io::Error| format!("{}: {e}", p.display());
    let includes = release.join("wp-includes");
    let out_includes = out.join("wp-includes");
    std::fs::create_dir_all(&out_includes).map_err(|e| err(&out_includes, e))?;
    for entry in std::fs::read_dir(release).map_err(|e| err(release, e))? {
        let entry = entry.map_err(|e| err(release, e))?;
        if entry.file_name() != "wp-includes" {
            link(&entry.path(), &out.join(entry.file_name()))?;
        }
    }
    for entry in std::fs::read_dir(&includes).map_err(|e| err(&includes, e))? {
        let entry = entry.map_err(|e| err(&includes, e))?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name != "compat.php" && !STUBBED.contains(&name.as_ref()) {
            link(&entry.path(), &out_includes.join(&*name))?;
        }
    }
    let compat = includes.join("compat.php");
    let stripped = strip_ported(&std::fs::read_to_string(&compat).map_err(|e| err(&compat, e))?)?;
    write_if_changed(&out_includes.join("compat.php"), &stripped)?;
    for name in STUBBED {
        write_if_changed(&out_includes.join(name), STUB)?;
    }
    Ok(out.to_path_buf())
}

fn write_if_changed(path: &Path, text: &str) -> Result<(), String> {
    if std::fs::read_to_string(path).ok().as_deref() == Some(text) {
        return Ok(());
    }
    let temp = path.with_extension(format!("php.{}", std::process::id()));
    std::fs::write(&temp, text).map_err(|e| format!("{}: {e}", temp.display()))?;
    std::fs::rename(&temp, path).map_err(|e| format!("{}: {e}", path.display()))
}

fn link(from: &Path, to: &Path) -> Result<(), String> {
    if to.exists() || to.symlink_metadata().is_ok() {
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
