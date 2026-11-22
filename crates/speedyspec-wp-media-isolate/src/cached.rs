//! `CachedReader`: a shared, content-keyed result cache in front of any reader (Decorator).
//!
//! Plugins often read a file's facts at render time instead of storing them, so a probe child per call would be paid
//! again and again. Results go into the shared Caching Store (files by default, Redis or the database with
//! `[cluster]`), keyed by what identifies the file's content, with a small per-worker LRU in front.

use std::cell::{Cell, RefCell};
use std::fs::File;
use std::io::Read;
use std::num::NonZeroUsize;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};

use lru::LruCache;
use sha2::{Digest, Sha256};
use speedyspec_cache::CachingStore;
use speedyspec_wp_media_contracts::{
    Limit, MediaInfo, MediaReader, MediaSource, ProbeLimits, ReadError,
};

use crate::{log, wire};

const GROUP: &str = "speedyspec_media";
const HEAD: u64 = 64 * 1024;
/// How long a failure that may not repeat (a timeout, a crash, a CPU or memory limit) is remembered.
const TRANSIENT_TTL: Duration = Duration::from_secs(300);

/// Caches reads of paths. Descriptors bypass it (they have no stable identity). Failures are cached too, so one bad
/// upload doesn't start a probe child on every page view: failures the same bytes always produce (unrecognized,
/// truncated, a byte, stream or output limit) for the full TTL, failures that may not repeat (a timeout, a crash, a
/// CPU or memory limit) for five minutes. A missing or unreadable file is never cached.
pub struct CachedReader<R: MediaReader> {
    inner: R,
    store: Rc<dyn CachingStore>,
    root: PathBuf,
    ttl: Duration,
    local: RefCell<LruCache<String, (Vec<u8>, Instant)>>,
    failures_logged: Cell<u32>,
}

impl<R: MediaReader> CachedReader<R> {
    /// The cache over `inner`, keying paths relative to `root` (the app's directory), keeping entries for `ttl` and
    /// `local_entries` decoded results per worker. Spec defaults: 30 days, 256 entries.
    pub fn new(
        inner: R,
        store: Rc<dyn CachingStore>,
        root: PathBuf,
        ttl: Duration,
        local_entries: NonZeroUsize,
    ) -> Self {
        // Canonical, so a root reached through a symlink still prefixes the canonical paths callers pass.
        let root = std::fs::canonicalize(&root).unwrap_or(root);
        CachedReader {
            inner,
            store,
            root,
            ttl,
            local: RefCell::new(LruCache::new(local_entries)),
            failures_logged: Cell::new(0),
        }
    }

    /// How long the outcome `result` is cached, given the cache's TTL for successes; `None` for never.
    pub fn ttl_for(result: &Result<MediaInfo, ReadError>, ttl: Duration) -> Option<Duration> {
        match result {
            Ok(_)
            | Err(ReadError::Unrecognized)
            | Err(ReadError::Truncated)
            | Err(ReadError::LimitExceeded(Limit::Bytes | Limit::Streams | Limit::Output)) => {
                Some(ttl)
            }
            Err(ReadError::TimedOut)
            | Err(ReadError::Crashed(_))
            | Err(ReadError::LimitExceeded(_)) => Some(TRANSIENT_TTL.min(ttl)),
            Err(_) => None,
        }
    }

    /// How many store failures were logged (once per reader, however many happen).
    pub fn store_failures_logged(&self) -> u32 {
        self.failures_logged.get()
    }

    fn store_failed(&self, err: &dyn std::fmt::Display) {
        if self.failures_logged.get() == 0 {
            log::event(
                "media_cache_unavailable",
                &[("store", self.store.name()), ("error", &err.to_string())],
            );
            self.failures_logged.set(1);
        }
    }

    /// The content key of `path`: its path relative to the root, size, modification time and the hash of its first
    /// 64 KiB. `None` when the path isn't a readable regular file under the root (it is then read uncached).
    fn key(&self, path: &Path) -> Option<String> {
        let meta = std::fs::symlink_metadata(path)
            .ok()
            .filter(|m| m.file_type().is_file())?;
        let mut head = Vec::new();
        File::options()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(path)
            .ok()?
            .take(HEAD)
            .read_to_end(&mut head)
            .ok()?;
        // Only files under the app's root: anything else (a stream wrapper's temporary copy, a new path every time)
        // would leave entries nothing can hit.
        // The directory is canonicalized, the final component kept as given (a symlinked file is refused by the
        // reader, not resolved here).
        let canonical = std::fs::canonicalize(path.parent()?)
            .ok()?
            .join(path.file_name()?);
        let relative = canonical.strip_prefix(&self.root).ok()?;
        let mtime_ns = i128::from(meta.mtime()) * 1_000_000_000 + i128::from(meta.mtime_nsec());
        let mut hasher = Sha256::new();
        hasher.update(relative.as_os_str().as_encoded_bytes());
        hasher.update(b"\0");
        hasher.update(meta.size().to_le_bytes());
        hasher.update(b"\0");
        hasher.update(mtime_ns.to_le_bytes());
        hasher.update(b"\0");
        hasher.update(Sha256::digest(&head));
        Some(
            hasher
                .finalize()
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect(),
        )
    }
}

fn decoded(bytes: &[u8]) -> Option<Result<MediaInfo, ReadError>> {
    wire::decode(bytes).ok()
}

impl<R: MediaReader> MediaReader for CachedReader<R> {
    fn read(&self, source: MediaSource, limits: &ProbeLimits) -> Result<MediaInfo, ReadError> {
        let MediaSource::Path(path) = source else {
            return self.inner.read(source, limits);
        };
        let Some(key) = self.key(&path) else {
            return self.inner.read(MediaSource::Path(path), limits);
        };
        let local_hit = match self.local.borrow_mut().get(&key) {
            Some((bytes, expires)) if *expires > Instant::now() => decoded(bytes),
            _ => None,
        };
        if let Some(result) = local_hit {
            return result;
        }
        match self.store.get(GROUP, &key) {
            Ok(Some(bytes)) => {
                if let Some(result) = decoded(&bytes) {
                    if let Some(ttl) = Self::ttl_for(&result, self.ttl) {
                        self.local
                            .borrow_mut()
                            .put(key, (bytes, Instant::now() + ttl));
                    }
                    return result;
                }
            }
            Ok(None) => {}
            Err(err) => self.store_failed(&err),
        }
        let result = self.inner.read(MediaSource::Path(path.clone()), limits);
        // A file that changed while it was read is not cached under the key it had before.
        if let Some(ttl) = Self::ttl_for(&result, self.ttl)
            && self.key(&path).as_deref() == Some(key.as_str())
        {
            let bytes = wire::encode(&result);
            if let Err(err) = self.store.set(GROUP, &key, &bytes, Some(ttl)) {
                self.store_failed(&err);
            }
            self.local
                .borrow_mut()
                .put(key, (bytes, Instant::now() + ttl));
        }
        result
    }
}
