//! The file system as `FileProbe` asks it.

use std::path::{Path, PathBuf};

use speedyspec_wp_i18n_contracts::{FileIdentity, FileProbe};

/// `realpath()`, `is_readable()`, `stat()` and a one-directory `glob()` over the real file system.
#[derive(Clone, Copy, Debug, Default)]
pub struct StdFileProbe;

impl FileProbe for StdFileProbe {
    fn realpath(&self, path: &Path) -> Option<PathBuf> {
        std::fs::canonicalize(path).ok()
    }

    fn is_readable(&self, path: &Path) -> bool {
        std::fs::File::open(path).is_ok()
    }

    fn identity(&self, path: &Path) -> Option<FileIdentity> {
        let meta = std::fs::metadata(path).ok()?;
        #[cfg(unix)]
        let (dev, ino, mtime_ns) = {
            use std::os::unix::fs::MetadataExt;
            (
                meta.dev(),
                meta.ino(),
                i128::from(meta.mtime()) * 1_000_000_000 + i128::from(meta.mtime_nsec()),
            )
        };
        #[cfg(not(unix))]
        let (dev, ino, mtime_ns) = (
            0,
            0,
            meta.modified()
                .ok()?
                .duration_since(std::time::UNIX_EPOCH)
                .ok()?
                .as_nanos() as i128,
        );
        Some(FileIdentity {
            path: path.to_path_buf(),
            dev,
            ino,
            size: meta.len(),
            mtime_ns,
        })
    }

    fn glob(&self, dir: &Path, pattern: &str) -> Vec<PathBuf> {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return Vec::new();
        };
        let mut found: Vec<PathBuf> = entries
            .filter_map(|e| e.ok())
            .filter(|e| matches(pattern.as_bytes(), e.file_name().as_encoded_bytes()))
            .map(|e| dir.join(e.file_name()))
            .collect();
        found.sort();
        found
    }
}

/// `*` matches any run of characters; everything else matches itself. Like `glob()`, a leading `.` is only matched
/// by a pattern that starts with one.
fn matches(pattern: &[u8], name: &[u8]) -> bool {
    if name.first() == Some(&b'.') && pattern.first() != Some(&b'.') {
        return false;
    }
    fn go(p: &[u8], n: &[u8]) -> bool {
        match p.split_first() {
            None => n.is_empty(),
            Some((b'*', rest)) => (0..=n.len()).any(|i| go(rest, &n[i..])),
            Some((c, rest)) => n.first() == Some(c) && go(rest, &n[1..]),
        }
    }
    go(pattern, name)
}
