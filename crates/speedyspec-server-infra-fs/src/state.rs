//! `state.json`, written atomically under a lock.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use speedyspec_server_domain::{ReleaseState, StateError, StateStore};

/// The release state in `<data_dir>/state.json`. Every save takes an exclusive lock on `<run_dir>/state.lock`, writes
/// `state.json.tmp`, fsyncs it, renames it over `state.json`, and fsyncs the directory.
pub struct FsStateStore {
    path: PathBuf,
    dir: PathBuf,
    lock: PathBuf,
    /// The last copy that parsed: a torn read (a network file system mid-rename) answers with it.
    last_good: std::sync::Mutex<Option<ReleaseState>>,
    unreadable: std::sync::atomic::AtomicBool,
}

/// Distinguishes this process's temp files from every other writer's on a shared volume.
static TEMP: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

impl FsStateStore {
    pub fn new(data_dir: &Path, run_dir: &Path) -> FsStateStore {
        FsStateStore {
            path: data_dir.join("state.json"),
            dir: data_dir.to_path_buf(),
            lock: run_dir.join("state.lock"),
            last_good: std::sync::Mutex::new(None),
            unreadable: std::sync::atomic::AtomicBool::new(false),
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
            Ok(bytes) => match serde_json::from_slice::<ReleaseState>(&bytes) {
                Ok(state) => {
                    self.unreadable
                        .store(false, std::sync::atomic::Ordering::SeqCst);
                    *self.last_good.lock().unwrap_or_else(|p| p.into_inner()) = Some(state.clone());
                    Ok(state)
                }
                Err(err) => {
                    let last = self
                        .last_good
                        .lock()
                        .unwrap_or_else(|p| p.into_inner())
                        .clone();
                    match last {
                        Some(state) => {
                            if !self
                                .unreadable
                                .swap(true, std::sync::atomic::Ordering::SeqCst)
                            {
                                tracing::warn!(event = "state_unreadable", path = %self.path.display(), error = %err, "keeping the last good copy");
                            }
                            Ok(state)
                        }
                        None => Err(self.error(&self.path, err)),
                    }
                }
            },
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
        // Each writer its own temp file: the lock above is pod-local, so two pods' saves must not share one.
        let tmp_path = self.dir.join(format!(
            ".state.json.{}.{}.tmp",
            std::process::id(),
            TEMP.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
        ));
        let mut tmp = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp_path)
            .map_err(|err| self.error(&tmp_path, err))?;
        let written = tmp
            .write_all(&json)
            .and_then(|()| tmp.sync_all())
            .and_then(|()| std::fs::rename(&tmp_path, &self.path));
        drop(tmp);
        if let Err(err) = written {
            let _ = std::fs::remove_file(&tmp_path);
            return Err(self.error(&self.path, err));
        }
        *self.last_good.lock().unwrap_or_else(|p| p.into_inner()) = Some(state.clone());
        if let Some(dir) = self.path.parent() {
            File::open(dir)
                .and_then(|d| d.sync_all())
                .map_err(|err| self.error(dir, err))?;
        }
        drop(lock);

        Ok(())
    }
}
