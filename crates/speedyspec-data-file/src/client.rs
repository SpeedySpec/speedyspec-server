//! `FileClient`: file operations under one root directory, by relative path only.

use std::cell::Cell;
use std::fs::{self, File, OpenOptions};
use std::io::{ErrorKind, Write};
use std::os::fd::AsRawFd;
use std::path::{Component, Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};

use speedyspec_data::DbError;

/// A temp file name's counter within the process.
static COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Clone)]
pub struct FileClient {
    root: PathBuf,
    engine: String,
    connected: Rc<Cell<bool>>,
}

/// An exclusive `flock` on a file, released when dropped.
pub struct FileLock {
    file: File,
}

impl Drop for FileLock {
    fn drop(&mut self) {
        // SAFETY: the descriptor is the open file's own.
        unsafe { libc::flock(self.file.as_raw_fd(), libc::LOCK_UN) };
    }
}

impl FileClient {
    pub(crate) fn new(engine: &str, root: PathBuf, connected: Rc<Cell<bool>>) -> FileClient {
        FileClient {
            root,
            engine: engine.to_string(),
            connected,
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn io(&self, path: &str, e: std::io::Error) -> DbError {
        DbError::Engine {
            engine: self.engine.clone(),
            code: Some(format!("{:?}", e.kind())),
            message: format!("{path}: {e}"),
        }
    }

    /// The file `path` names under the root; only plain relative components are accepted, so a path never leaves
    /// the root.
    fn resolve(&self, path: &str) -> Result<PathBuf, DbError> {
        if !self.connected.get() {
            return Err(DbError::Connection {
                engine: self.engine.clone(),
                message: "the file store is closed".into(),
            });
        }
        let invalid = || DbError::Engine {
            engine: self.engine.clone(),
            code: Some("invalid-path".into()),
            message: format!(
                "`{}` is not a relative path of plain names",
                path.escape_debug()
            ),
        };
        if path.is_empty()
            || path.contains('\0')
            || path
                .split('/')
                .any(|part| part.is_empty() || part == "." || part == "..")
        {
            return Err(invalid());
        }
        let relative = Path::new(path);
        if !relative
            .components()
            .all(|c| matches!(c, Component::Normal(_)))
        {
            return Err(invalid());
        }
        Ok(self.root.join(relative))
    }

    pub fn read(&self, path: &str) -> Result<Option<Vec<u8>>, DbError> {
        let full = self.resolve(path)?;
        match fs::read(&full) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(e) if e.kind() == ErrorKind::NotFound => Ok(None),
            Err(e) => Err(self.io(path, e)),
        }
    }

    /// Writes through a temp file renamed over the target, so a reader sees the old file or the new one whole.
    pub fn write(&self, path: &str, bytes: &[u8]) -> Result<(), DbError> {
        let full = self.resolve(path)?;
        let dir = full.parent().expect("a resolved path has a parent");
        fs::create_dir_all(dir).map_err(|e| self.io(path, e))?;
        let name = full
            .file_name()
            .expect("a resolved path names a file")
            .to_string_lossy()
            .into_owned();
        let temp = dir.join(format!(
            ".{name}.tmp-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        let result = (|| {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temp)?;
            file.write_all(bytes)?;
            file.sync_data()?;
            fs::rename(&temp, &full)
        })();
        if let Err(e) = result {
            let _ = fs::remove_file(&temp);
            return Err(self.io(path, e));
        }
        Ok(())
    }

    pub fn remove(&self, path: &str) -> Result<bool, DbError> {
        let full = self.resolve(path)?;
        match fs::remove_file(&full) {
            Ok(()) => Ok(true),
            Err(e) if e.kind() == ErrorKind::NotFound => Ok(false),
            Err(e) => Err(self.io(path, e)),
        }
    }

    pub fn remove_dir_all(&self, path: &str) -> Result<bool, DbError> {
        let full = self.resolve(path)?;
        match fs::remove_dir_all(&full) {
            Ok(()) => Ok(true),
            Err(e) if e.kind() == ErrorKind::NotFound => Ok(false),
            Err(e) => Err(self.io(path, e)),
        }
    }

    pub fn rename(&self, from: &str, to: &str) -> Result<bool, DbError> {
        let (source, target) = (self.resolve(from)?, self.resolve(to)?);
        if let Some(dir) = target.parent() {
            fs::create_dir_all(dir).map_err(|e| self.io(to, e))?;
        }
        match fs::rename(&source, &target) {
            Ok(()) => Ok(true),
            Err(e) if e.kind() == ErrorKind::NotFound => Ok(false),
            Err(e) => Err(self.io(from, e)),
        }
    }

    /// The names in `dir`, sorted; empty when it does not exist.
    pub fn list(&self, dir: &str) -> Result<Vec<String>, DbError> {
        let full = self.resolve(dir)?;
        let entries = match fs::read_dir(&full) {
            Ok(entries) => entries,
            Err(e) if e.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(self.io(dir, e)),
        };
        let mut names = entries
            .map(|e| e.map(|e| e.file_name().to_string_lossy().into_owned()))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| self.io(dir, e))?;
        names.sort();
        Ok(names)
    }

    /// An exclusive lock on `path` (created if missing), held until the `FileLock` is dropped.
    pub fn lock(&self, path: &str) -> Result<FileLock, DbError> {
        let full = self.resolve(path)?;
        if let Some(dir) = full.parent() {
            fs::create_dir_all(dir).map_err(|e| self.io(path, e))?;
        }
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&full)
            .map_err(|e| self.io(path, e))?;
        // SAFETY: the descriptor is the open file's own.
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) } != 0 {
            return Err(self.io(path, std::io::Error::last_os_error()));
        }
        Ok(FileLock { file })
    }
}
