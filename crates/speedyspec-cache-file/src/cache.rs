//! `FileCache`: one file per entry, holding 8 bytes little-endian `expires_at` (Unix ms, `0` = never), 4 bytes
//! little-endian length of `group NUL key`, those bytes, then the value.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use speedyspec_cache::{CacheError, CachingStore};
use speedyspec_data::{DbError, DirectStore};
use speedyspec_data_file::{FileClient, FileLock};

use crate::layout::{entry_path, group_dir, lock_path, name, named_lock_path};

pub struct FileCache {
    client: FileClient,
    dir: String,
}

/// A live entry: its expiry and value.
type Entry = (u64, Vec<u8>);

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// When an expiry ends; under a millisecond lives a millisecond, and a far one saturates to the end of time.
fn expires_at(expiry: Option<Duration>) -> u64 {
    expiry
        .map(|e| now_ms().saturating_add(u64::try_from(e.as_millis()).unwrap_or(u64::MAX).max(1)))
        .unwrap_or(0)
}

fn encode(expires: u64, name: &[u8], value: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(12 + name.len() + value.len());
    out.extend(expires.to_le_bytes());
    out.extend((name.len() as u32).to_le_bytes());
    out.extend(name);
    out.extend(value);
    out
}

/// The entry's expiry and value when the file records `name`; a short header or another name is `None`.
fn decode(bytes: &[u8], name: &[u8]) -> Option<(u64, Vec<u8>)> {
    let (expires, rest) = bytes.split_first_chunk::<8>()?;
    let (len, rest) = rest.split_first_chunk::<4>()?;
    let len = u32::from_le_bytes(*len) as usize;
    let (stored, value) = (rest.get(..len)?, rest.get(len..)?);
    (stored == name).then(|| (u64::from_le_bytes(*expires), value.to_vec()))
}

fn number(group: &str, key: &str, value: &[u8]) -> Result<i64, CacheError> {
    std::str::from_utf8(value)
        .ok()
        .and_then(|s| s.parse::<i64>().ok())
        .ok_or_else(|| CacheError::NotANumber {
            group: group.into(),
            key: key.into(),
        })
}

impl FileCache {
    /// A cache in `dir` under the file store's root; any store but the file Direct Store is refused.
    pub fn new(store: &dyn DirectStore, dir: &str) -> Result<FileCache, CacheError> {
        let backend = |e: DbError| CacheError::Backend {
            cache: "file".into(),
            message: e.to_string(),
        };
        let client = store
            .as_any()
            .downcast_ref::<FileClient>()
            .cloned()
            .ok_or_else(|| {
                backend(DbError::Config(
                    "FileCache needs a file Direct Store".into(),
                ))
            })?;
        if dir.is_empty()
            || dir
                .split('/')
                .any(|p| p.is_empty() || p == "." || p == "..")
        {
            return Err(backend(DbError::Config(format!(
                "the cache directory `{dir}` is not a relative path"
            ))));
        }
        Ok(FileCache {
            client,
            dir: dir.to_string(),
        })
    }

    fn backend(&self, e: DbError) -> CacheError {
        CacheError::Backend {
            cache: "file".into(),
            message: e.to_string(),
        }
    }

    /// The live entry. An expired one is removed only under the group's lock (`locked`): a read without it could
    /// remove an entry a locked writer has just replaced.
    fn read(&self, group: &str, key: &str, locked: bool) -> Result<Option<Entry>, CacheError> {
        let path = entry_path(&self.dir, group, key);
        let Some(bytes) = self.client.read(&path).map_err(|e| self.backend(e))? else {
            return Ok(None);
        };
        match decode(&bytes, &name(group, key)) {
            Some((expires, _)) if expires != 0 && now_ms() >= expires => {
                if locked {
                    self.client.remove(&path).map_err(|e| self.backend(e))?;
                }
                Ok(None)
            }
            other => Ok(other),
        }
    }

    fn write(&self, group: &str, key: &str, expires: u64, value: &[u8]) -> Result<(), CacheError> {
        let path = entry_path(&self.dir, group, key);
        self.client
            .write_unsynced(&path, &encode(expires, &name(group, key), value))
            .map_err(|e| self.backend(e))
    }

    /// Stores `value`; a zero expiry removes the entry, as stored and expired at once.
    fn store(
        &self,
        group: &str,
        key: &str,
        value: &[u8],
        expiry: Option<Duration>,
    ) -> Result<(), CacheError> {
        if expiry.is_some_and(|e| e.is_zero()) {
            self.client
                .remove(&entry_path(&self.dir, group, key))
                .map_err(|e| self.backend(e))?;
            return Ok(());
        }
        self.write(group, key, expires_at(expiry), value)
    }

    /// The group's lock, held for a read-modify-write, a flush or a sweep so two processes never interleave on one
    /// group. It lives outside the group's directory, so a flush that moves the directory aside keeps every holder
    /// on the same lock.
    fn lock(&self, group: &str) -> Result<FileLock, CacheError> {
        self.client
            .lock(&lock_path(&self.dir, group))
            .map_err(|e| self.backend(e))
    }

    /// The lock of the group whose directory is `name` (`g-…`), for the flush and the sweep, which see only names.
    fn lock_named(&self, name: &str) -> Result<FileLock, CacheError> {
        self.client
            .lock(&named_lock_path(&self.dir, name))
            .map_err(|e| self.backend(e))
    }

    /// Moves `group_dir` aside, so readers see it vanish whole, then removes it.
    fn discard(&self, group_dir: &str) -> Result<(), CacheError> {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let trash = format!("{}/.trash-{}-{nanos}", self.dir, std::process::id());
        if self
            .client
            .rename(group_dir, &trash)
            .map_err(|e| self.backend(e))?
        {
            self.client
                .remove_dir_all(&trash)
                .map_err(|e| self.backend(e))?;
        }
        Ok(())
    }

    fn adjust(
        &self,
        group: &str,
        key: &str,
        f: impl FnOnce(i64) -> i64,
    ) -> Result<Option<i64>, CacheError> {
        let _lock = self.lock(group)?;
        let Some((expires, value)) = self.read(group, key, true)? else {
            return Ok(None);
        };
        let n = f(number(group, key, &value)?);
        // The entry keeps its expiry.
        self.write(group, key, expires, n.to_string().as_bytes())?;
        Ok(Some(n))
    }
}

impl FileCache {
    /// Removes every expired entry, read again or not; how many. The runtime calls it now and then, as WordPress's
    /// cron deletes expired transients.
    pub fn purge_expired(&self) -> Result<u64, CacheError> {
        let now = now_ms();
        let mut removed = 0;
        let list = |dir: &str| self.client.list(dir).map_err(|e| self.backend(e));
        for name in list(&self.dir)?.into_iter().filter(|g| g.starts_with("g-")) {
            let _lock = self.lock_named(&name)?;
            let group = format!("{}/{name}", self.dir);
            for shard in list(&group)?.into_iter().filter(|s| !s.starts_with('.')) {
                let shard = format!("{group}/{shard}");
                for entry in list(&shard)?.into_iter().filter(|e| !e.starts_with('.')) {
                    let path = format!("{shard}/{entry}");
                    let Some(bytes) = self.client.read(&path).map_err(|e| self.backend(e))? else {
                        continue;
                    };
                    let expires = bytes
                        .first_chunk::<8>()
                        .map(|b| u64::from_le_bytes(*b))
                        .unwrap_or(0);
                    if expires != 0
                        && now >= expires
                        && self.client.remove(&path).map_err(|e| self.backend(e))?
                    {
                        removed += 1;
                    }
                }
            }
        }
        Ok(removed)
    }
}

impl CachingStore for FileCache {
    fn name(&self) -> &str {
        "file"
    }

    fn get(&self, group: &str, key: &str) -> Result<Option<Vec<u8>>, CacheError> {
        Ok(self.read(group, key, false)?.map(|(_, v)| v))
    }

    fn get_many(&self, group: &str, keys: &[&str]) -> Result<Vec<Option<Vec<u8>>>, CacheError> {
        keys.iter().map(|k| self.get(group, k)).collect()
    }

    fn set(
        &self,
        group: &str,
        key: &str,
        value: &[u8],
        expiry: Option<Duration>,
    ) -> Result<(), CacheError> {
        self.store(group, key, value, expiry)
    }

    fn add(
        &self,
        group: &str,
        key: &str,
        value: &[u8],
        expiry: Option<Duration>,
    ) -> Result<bool, CacheError> {
        let _lock = self.lock(group)?;
        if self.read(group, key, true)?.is_some() {
            return Ok(false);
        }
        self.store(group, key, value, expiry)?;
        Ok(true)
    }

    fn replace(
        &self,
        group: &str,
        key: &str,
        value: &[u8],
        expiry: Option<Duration>,
    ) -> Result<bool, CacheError> {
        let _lock = self.lock(group)?;
        if self.read(group, key, true)?.is_none() {
            return Ok(false);
        }
        self.store(group, key, value, expiry)?;
        Ok(true)
    }

    fn delete(&self, group: &str, key: &str) -> Result<bool, CacheError> {
        let live = self.read(group, key, false)?.is_some();
        self.client
            .remove(&entry_path(&self.dir, group, key))
            .map_err(|e| self.backend(e))?;
        Ok(live)
    }

    fn increment(&self, group: &str, key: &str, by: i64) -> Result<Option<i64>, CacheError> {
        self.adjust(group, key, |n| n.saturating_add(by))
    }

    /// Floors at 0, as memcached does.
    fn decrement(&self, group: &str, key: &str, by: i64) -> Result<Option<i64>, CacheError> {
        self.adjust(group, key, |n| n.saturating_sub(by).max(0))
    }

    fn flush(&self) -> Result<(), CacheError> {
        for entry in self.client.list(&self.dir).map_err(|e| self.backend(e))? {
            if entry.starts_with("g-") {
                let _lock = self.lock_named(&entry)?;
                self.discard(&format!("{}/{entry}", self.dir))?;
            } else if entry.starts_with(".trash-") {
                // A flush a crash interrupted.
                self.client
                    .remove_dir_all(&format!("{}/{entry}", self.dir))
                    .map_err(|e| self.backend(e))?;
            }
        }
        Ok(())
    }

    fn flush_group(&self, group: &str) -> Result<(), CacheError> {
        let _lock = self.lock(group)?;
        self.discard(&group_dir(&self.dir, group))
    }
}
