//! A WordPress release prepared for the native token map: every file linked to the release's, and the files the
//! facade declares replaced by stubs.

use std::path::{Path, PathBuf};

use speedyspec_compat::ReleaseMirror;

/// Files in `wp-includes/` the facade declares.
const STUBBED: &[&str] = &["class-wp-token-map.php"];

const STUB: &str = "<?php\n// Provided by speedyspec-wp-token-map-php.\n";

/// Prepares `release` in `out` for the native token map. Returns `out`.
pub fn prepare_release(release: &Path, out: &Path) -> Result<PathBuf, String> {
    let mirror = ReleaseMirror::new(release, out)?;
    for name in STUBBED {
        mirror.write(&format!("wp-includes/{name}"), STUB)?;
    }
    Ok(out.to_path_buf())
}
