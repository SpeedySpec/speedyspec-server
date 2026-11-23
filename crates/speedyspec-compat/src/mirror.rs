//! A WordPress release mirrored for one port's PHPUnit run: every entry links to the release's, except the files the
//! port rewrites (its ported functions stripped, or stubbed), and the directories holding them, which are real so
//! their other entries can be links.
//!
//! The release is unpacked once per machine and shared by every compat suite, so nothing may ever be written through
//! a link into it. A mirror is reused between runs and between branches whose ports rewrite different files: a
//! directory an earlier run linked whole is made real before anything is written under it, and a file an earlier run
//! linked is replaced, never written through.

use std::path::{Path, PathBuf};

/// A release (`wordpress/` root) mirrored into `out`.
#[derive(Debug, Clone)]
pub struct ReleaseMirror {
    release: PathBuf,
    out: PathBuf,
}

impl ReleaseMirror {
    /// Mirrors `release` into `out`, linking every top-level entry `out` does not already hold.
    pub fn new(release: &Path, out: &Path) -> Result<Self, String> {
        let mirror = Self {
            release: release.to_path_buf(),
            out: out.to_path_buf(),
        };
        mirror.make_real(Path::new(""))?;
        Ok(mirror)
    }

    /// The mirrored root, which WordPress's tests load as `ABSPATH`.
    pub fn root(&self) -> &Path {
        &self.out
    }

    /// The release's own text of `relative`.
    pub fn read(&self, relative: &str) -> Result<String, String> {
        let source = self.release.join(relative);
        std::fs::read_to_string(&source).map_err(|e| err(&source, e))
    }

    /// Writes `text` at `relative` (a `/`-separated path under the root), making every directory on the way real
    /// first, and replacing a link left there rather than writing through it. Leaves a file whose text is already
    /// `text` alone.
    pub fn write(&self, relative: &str, text: &str) -> Result<(), String> {
        let relative = Path::new(relative);
        let mut dir = PathBuf::new();
        if let Some(parent) = relative.parent() {
            for part in parent.components() {
                dir.push(part);
                self.make_real(&dir)?;
            }
        }
        let target = self.out.join(relative);
        let linked = is_link(&target);
        if !linked && std::fs::read_to_string(&target).ok().as_deref() == Some(text) {
            return Ok(());
        }
        if linked {
            std::fs::remove_file(&target).map_err(|e| err(&target, e))?;
        }
        let name = relative
            .file_name()
            .ok_or_else(|| format!("{} names no file", relative.display()))?
            .to_string_lossy();
        let staged = target.with_file_name(format!("{name}.{}.partial", std::process::id()));
        std::fs::write(&staged, text).map_err(|e| err(&staged, e))?;
        std::fs::rename(&staged, &target).map_err(|e| err(&target, e))
    }

    /// Makes the directory `relative` real in the mirror (replacing a link to the release's) and links each of the
    /// release's entries in it that the mirror does not already hold.
    fn make_real(&self, relative: &Path) -> Result<(), String> {
        let target = self.out.join(relative);
        if is_link(&target) {
            std::fs::remove_file(&target).map_err(|e| err(&target, e))?;
        }
        std::fs::create_dir_all(&target).map_err(|e| err(&target, e))?;
        let source = self.release.join(relative);
        for entry in std::fs::read_dir(&source).map_err(|e| err(&source, e))? {
            let entry = entry.map_err(|e| err(&source, e))?;
            link(&entry.path(), &target.join(entry.file_name()))?;
        }
        Ok(())
    }
}

fn is_link(path: &Path) -> bool {
    path.symlink_metadata()
        .is_ok_and(|m| m.file_type().is_symlink())
}

fn link(from: &Path, to: &Path) -> Result<(), String> {
    if to.symlink_metadata().is_ok() {
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
    made.map_err(|e| err(to, e))
}

fn err(path: &Path, e: std::io::Error) -> String {
    format!("{}: {e}", path.display())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn release() -> tempfile::TempDir {
        let release = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(release.path().join("a/b")).unwrap();
        std::fs::write(release.path().join("index.php"), "index").unwrap();
        std::fs::write(release.path().join("a/f.php"), "original").unwrap();
        std::fs::write(release.path().join("a/g.php"), "other").unwrap();
        std::fs::write(release.path().join("a/b/h.php"), "deep").unwrap();
        release
    }

    fn read(path: &Path) -> String {
        std::fs::read_to_string(path).unwrap()
    }

    /// A mirror prepared before a file under one of its linked directories was rewritten must not write through that
    /// link into the release (which every compat suite shares).
    #[test]
    fn a_linked_directory_is_made_real_before_writing_under_it() {
        let release = release();
        let out = tempfile::tempdir().unwrap();
        let mirror = ReleaseMirror::new(release.path(), out.path()).unwrap();
        assert!(is_link(&out.path().join("a")), "a is linked whole at first");

        mirror.write("a/b/h.php", "stripped").unwrap();

        assert_eq!(read(&release.path().join("a/b/h.php")), "deep");
        assert_eq!(read(&out.path().join("a/b/h.php")), "stripped");
        assert!(!is_link(&out.path().join("a")) && !is_link(&out.path().join("a/b")));
        assert!(is_link(&out.path().join("a/g.php")), "siblings stay links");
        assert_eq!(read(&out.path().join("a/g.php")), "other");
        assert_eq!(read(&out.path().join("index.php")), "index");
    }

    /// A file an earlier run linked is replaced, not written through, even when the stripped text is unchanged.
    #[test]
    fn a_linked_file_is_replaced_not_written_through() {
        let release = release();
        let out = tempfile::tempdir().unwrap();
        let mirror = ReleaseMirror::new(release.path(), out.path()).unwrap();
        mirror.write("a/g.php", "other").unwrap(); // makes a/ real; a/f.php is a link
        assert!(is_link(&out.path().join("a/f.php")));

        mirror.write("a/f.php", "original").unwrap();

        assert!(!is_link(&out.path().join("a/f.php")));
        mirror.write("a/f.php", "stripped").unwrap();
        assert_eq!(read(&release.path().join("a/f.php")), "original");
        assert_eq!(read(&out.path().join("a/f.php")), "stripped");
    }

    /// A second mirror over the same folder reuses it and leaves earlier rewrites alone.
    #[test]
    fn a_mirror_is_reused() {
        let release = release();
        let out = tempfile::tempdir().unwrap();
        ReleaseMirror::new(release.path(), out.path())
            .unwrap()
            .write("a/f.php", "stripped")
            .unwrap();
        let again = ReleaseMirror::new(release.path(), out.path()).unwrap();
        assert_eq!(again.read("a/f.php").unwrap(), "original");
        assert_eq!(read(&again.root().join("a/f.php")), "stripped");
        assert!(std::fs::read_dir(out.path().join("a")).unwrap().all(|e| {
            !e.unwrap()
                .file_name()
                .to_string_lossy()
                .ends_with(".partial")
        }));
    }
}
