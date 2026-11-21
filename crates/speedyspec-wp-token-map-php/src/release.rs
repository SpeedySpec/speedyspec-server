//! A WordPress release prepared for the native token map: every file linked to the release's, and the files the
//! facade declares replaced by stubs.

use std::path::{Path, PathBuf};

/// Files in `wp-includes/` the facade declares.
const STUBBED: &[&str] = &[];

const STUB: &str = "<?php\n// Provided by speedyspec-wp-token-map-php.\n";

fn err(p: &Path, e: std::io::Error) -> String {
    format!("{}: {e}", p.display())
}

fn link(from: &Path, to: &Path) -> Result<(), String> {
    if to.symlink_metadata().is_ok() {
        return Ok(());
    }
    #[cfg(unix)]
    std::os::unix::fs::symlink(from, to).map_err(|e| err(to, e))?;
    Ok(())
}

fn write_if_changed(path: &Path, text: &str) -> Result<(), String> {
    if path
        .symlink_metadata()
        .is_ok_and(|m| m.file_type().is_symlink())
    {
        std::fs::remove_file(path).map_err(|e| err(path, e))?;
    }
    if std::fs::read_to_string(path).ok().as_deref() == Some(text) {
        return Ok(());
    }
    let temp = path.with_extension(format!("php.{}", std::process::id()));
    std::fs::write(&temp, text).map_err(|e| err(&temp, e))?;
    std::fs::rename(&temp, path).map_err(|e| err(path, e))
}

/// Prepares `release` in `out` for the native token map. Returns `out`.
pub fn prepare_release(release: &Path, out: &Path) -> Result<PathBuf, String> {
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
        if !STUBBED.contains(&name.to_string_lossy().as_ref()) {
            link(&entry.path(), &out_includes.join(&name))?;
        }
    }
    for name in STUBBED {
        write_if_changed(&out_includes.join(name), STUB)?;
    }
    Ok(out.to_path_buf())
}
