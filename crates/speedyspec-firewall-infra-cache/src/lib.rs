//! The firewall's counters and bans on a [`CachingStore`] (Adapter). Any Caching Store works, so the firewall's
//! state lives wherever the site's persistent cache lives: in memory, in the database, in Valkey or ReDB, or in
//! files.
//!
//! Counters use fixed windows: one entry per key and window, created with `add` and counted with `increment`.
//! On Valkey and ReDB, `KvCache`'s `increment` reads and then writes, so two workers counting the same key at
//! the same moment can undercount by one. That is acceptable for rate limiting.

use std::rc::Rc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use speedyspec_cache::{CacheError, CachingStore};
use speedyspec_firewall::{BanStore, Clock, CounterStore, FirewallError};

/// The cache group every firewall entry lives in.
pub const GROUP: &str = "speedyspec-firewall";

fn error(error: CacheError) -> FirewallError {
    FirewallError(error.to_string())
}

/// The real clock.
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> SystemTime {
        SystemTime::now()
    }
}

pub struct CacheCounterStore {
    cache: Rc<dyn CachingStore>,
    clock: Rc<dyn Clock>,
}

impl CacheCounterStore {
    pub fn new(cache: Rc<dyn CachingStore>, clock: Rc<dyn Clock>) -> CacheCounterStore {
        CacheCounterStore { cache, clock }
    }
}

impl CounterStore for CacheCounterStore {
    fn hit(&self, key: &str, window: Duration) -> Result<u64, FirewallError> {
        let seconds = self
            .clock
            .now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let entry = format!("{key}:{}", seconds / window.as_secs().max(1));
        for _ in 0..2 {
            self.cache
                .add(GROUP, &entry, b"0", Some(window * 2))
                .map_err(error)?;
            if let Some(count) = self.cache.increment(GROUP, &entry, 1).map_err(error)? {
                return Ok(count.max(0) as u64);
            }
        }
        Err(FirewallError(format!("the counter {entry} vanished twice")))
    }
}

pub struct CacheBanStore {
    cache: Rc<dyn CachingStore>,
}

impl CacheBanStore {
    pub fn new(cache: Rc<dyn CachingStore>) -> CacheBanStore {
        CacheBanStore { cache }
    }
}

impl BanStore for CacheBanStore {
    fn ban(&self, key: &str, for_: Duration, reason: &str) -> Result<(), FirewallError> {
        self.cache
            .set(GROUP, &format!("ban:{key}"), reason.as_bytes(), Some(for_))
            .map_err(error)
    }

    fn banned(&self, key: &str) -> Result<Option<String>, FirewallError> {
        let value = self
            .cache
            .get(GROUP, &format!("ban:{key}"))
            .map_err(error)?;
        Ok(value.map(|bytes| String::from_utf8_lossy(&bytes).into_owned()))
    }

    fn lift(&self, key: &str) -> Result<(), FirewallError> {
        self.cache
            .delete(GROUP, &format!("ban:{key}"))
            .map(|_| ())
            .map_err(error)
    }
}
