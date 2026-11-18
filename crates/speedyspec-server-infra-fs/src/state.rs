//! `state.json`, written atomically under a lock.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use speedyspec_server_domain::{ReleaseState, StateError, StateStore};

/// The release state in `<data_dir>/state.json`. Every save takes an exclusive lock on `<run_dir>/state.lock`, writes
/// `state.json.tmp`, fsyncs it, renames it over `state.json`, and fsyncs the directory.
pub struct FsStateStore {
    path: PathBuf,
    tmp: PathBuf,
    lock: PathBuf,
}

impl FsStateStore {
    pub fn new(data_dir: &Path, run_dir: &Path) -> FsStateStore {
        FsStateStore {
            path: data_dir.join("state.json"),
            tmp: data_dir.join("state.json.tmp"),
            lock: run_dir.join("state.lock"),
        }
    }

    fn error(&self, path: &Path, err: impl ToString) -> StateError {
        StateError {
            path: path.to_path_buf(),
            message: err.to_string(),
        }
    }
}

impl StateStore for FsStateStore {
    fn load(&self) -> Result<ReleaseState, StateError> {
        match std::fs::read(&self.path) {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(|err| self.error(&self.path, err)),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(ReleaseState::default()),
            Err(err) => Err(self.error(&self.path, err)),
        }
    }

    fn save(&self, state: &ReleaseState) -> Result<(), StateError> {
        if let Some(dir) = self.lock.parent() {
            std::fs::create_dir_all(dir).map_err(|err| self.error(dir, err))?;
        }
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&self.lock)
            .map_err(|err| self.error(&self.lock, err))?;
        lock.lock().map_err(|err| self.error(&self.lock, err))?;

        let json = serde_json::to_vec_pretty(state).map_err(|err| self.error(&self.path, err))?;
        let mut tmp = File::create(&self.tmp).map_err(|err| self.error(&self.tmp, err))?;
        tmp.write_all(&json)
            .map_err(|err| self.error(&self.tmp, err))?;
        tmp.sync_all().map_err(|err| self.error(&self.tmp, err))?;
        drop(tmp);
        std::fs::rename(&self.tmp, &self.path).map_err(|err| self.error(&self.path, err))?;
        if let Some(dir) = self.path.parent() {
            File::open(dir)
                .and_then(|d| d.sync_all())
                .map_err(|err| self.error(dir, err))?;
        }
        drop(lock);

        Ok(())
    }
}
