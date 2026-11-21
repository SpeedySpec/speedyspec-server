//! `KvCache` over a `NoSqlStore`.

use std::rc::Rc;
use std::time::Duration;

use speedyspec_cache::{CacheError, CachingStore};
use speedyspec_data::{DbError, NoSqlStore};
use speedyspec_data_counter::parse_counter;

use crate::keys;

/// Past this an expiry means never: the stores' clocks cannot hold WordPress's far-future expiries.
const NEVER: Duration = Duration::from_secs(100 * 365 * 24 * 60 * 60);

pub struct KvCache {
    store: Rc<dyn NoSqlStore>,
    keyspace: String,
    name: String,
}

fn ttl(expiry: Option<Duration>) -> Option<Duration> {
    expiry.filter(|e| *e <= NEVER)
}

/// WordPress's reading of a cached number: an ASCII signed decimal.
fn number(group: &str, key: &str, value: &[u8]) -> Result<i64, CacheError> {
    std::str::from_utf8(value)
        .ok()
        .and_then(|s| s.parse::<i64>().ok())
        .ok_or_else(|| CacheError::NotANumber {
            group: group.into(),
            key: key.into(),
        })
}

fn engine_code(e: &DbError) -> Option<&str> {
    match e {
        DbError::Engine { code: Some(c), .. } => Some(c),
        _ => None,
    }
}

impl KvCache {
    /// A cache in `keyspace` of `store`; give each site its own keyspace (`cache-<site>`).
    pub fn new(store: Rc<dyn NoSqlStore>, keyspace: &str) -> KvCache {
        let name = format!("kv:{}", store.engine());
        KvCache {
            store,
            keyspace: keyspace.to_string(),
            name,
        }
    }

    fn backend(&self, e: DbError) -> CacheError {
        CacheError::Backend {
            cache: self.name.clone(),
            message: e.to_string(),
        }
    }

    /// The group's current generation.
    fn generation(&self, group: &str) -> Result<u64, CacheError> {
        let bytes = self
            .store
            .get(&self.keyspace, &keys::generation(group.as_bytes()))
            .map_err(|e| self.backend(e))?;
        Ok(bytes
            .as_deref()
            .and_then(parse_counter)
            .map(|n| n.max(0) as u64)
            .unwrap_or(0))
    }

    fn entry(&self, group: &str, key: &str) -> Result<Vec<u8>, CacheError> {
        Ok(keys::entry(
            group.as_bytes(),
            self.generation(group)?,
            key.as_bytes(),
        ))
    }

    /// Adds `by` to a live entry in one atomic store call, floored at `floor` when given; an absent entry stays
    /// absent. A value the store cannot count (not canonical, or overflowing) is counted here.
    fn adjust(
        &self,
        group: &str,
        key: &str,
        by: i64,
        floor: Option<i64>,
    ) -> Result<Option<i64>, CacheError> {
        let entry = self.entry(group, key)?;
        match self
            .store
            .adjust_existing(&self.keyspace, &entry, by, floor)
        {
            Ok(n) => Ok(n),
            Err(e) if matches!(engine_code(&e), Some("not-integer" | "overflow")) => {
                let Some(current) = self
                    .store
                    .get(&self.keyspace, &entry)
                    .map_err(|e| self.backend(e))?
                else {
                    return Ok(None);
                };
                let n = number(group, key, &current)?.saturating_add(by);
                let n = floor.map_or(n, |f| n.max(f));
                // The store cannot report the entry's expiry, so a value it could not count is written without one.
                self.store
                    .put(&self.keyspace, &entry, n.to_string().as_bytes(), None)
                    .map_err(|e| self.backend(e))?;
                Ok(Some(n))
            }
            Err(e) => Err(self.backend(e)),
        }
    }
}

impl KvCache {
    /// Removes every entry of `group` under a generation below `current`, a batch at a time.
    fn purge_generations(&self, group: &str, current: u64) -> Result<(), DbError> {
        const BATCH: usize = 1000;
        let start = keys::entry(group.as_bytes(), 0, b"");
        let end = keys::entry(group.as_bytes(), current, b"");
        loop {
            let found = self.store.range(
                &self.keyspace,
                std::ops::Bound::Included(&start),
                std::ops::Bound::Excluded(&end),
                Some(BATCH),
            )?;
            for (k, _) in &found {
                self.store.delete(&self.keyspace, k)?;
            }
            if found.len() < BATCH {
                return Ok(());
            }
        }
    }
}

impl CachingStore for KvCache {
    fn name(&self) -> &str {
        &self.name
    }

    fn get(&self, group: &str, key: &str) -> Result<Option<Vec<u8>>, CacheError> {
        let entry = self.entry(group, key)?;
        self.store
            .get(&self.keyspace, &entry)
            .map_err(|e| self.backend(e))
    }

    fn get_many(&self, group: &str, keys: &[&str]) -> Result<Vec<Option<Vec<u8>>>, CacheError> {
        let generation = self.generation(group)?;
        let entries: Vec<Vec<u8>> = keys
            .iter()
            .map(|k| keys::entry(group.as_bytes(), generation, k.as_bytes()))
            .collect();
        let refs: Vec<&[u8]> = entries.iter().map(Vec::as_slice).collect();
        self.store
            .get_many(&self.keyspace, &refs)
            .map_err(|e| self.backend(e))
    }

    fn set(
        &self,
        group: &str,
        key: &str,
        value: &[u8],
        expiry: Option<Duration>,
    ) -> Result<(), CacheError> {
        let entry = self.entry(group, key)?;
        self.store
            .put(&self.keyspace, &entry, value, ttl(expiry))
            .map_err(|e| self.backend(e))
    }

    fn add(
        &self,
        group: &str,
        key: &str,
        value: &[u8],
        expiry: Option<Duration>,
    ) -> Result<bool, CacheError> {
        let entry = self.entry(group, key)?;
        if expiry.is_some_and(|e| e.is_zero()) {
            // Stored and expired at once: added when nothing live was there.
            return Ok(self
                .store
                .get(&self.keyspace, &entry)
                .map_err(|e| self.backend(e))?
                .is_none());
        }
        self.store
            .put_if_absent(&self.keyspace, &entry, value, ttl(expiry))
            .map_err(|e| self.backend(e))
    }

    /// Reads, then writes: not atomic across processes, as WordPress's own persistent drop-ins.
    fn replace(
        &self,
        group: &str,
        key: &str,
        value: &[u8],
        expiry: Option<Duration>,
    ) -> Result<bool, CacheError> {
        let entry = self.entry(group, key)?;
        if self
            .store
            .get(&self.keyspace, &entry)
            .map_err(|e| self.backend(e))?
            .is_none()
        {
            return Ok(false);
        }
        self.store
            .put(&self.keyspace, &entry, value, ttl(expiry))
            .map_err(|e| self.backend(e))?;
        Ok(true)
    }

    fn delete(&self, group: &str, key: &str) -> Result<bool, CacheError> {
        let entry = self.entry(group, key)?;
        self.store
            .delete(&self.keyspace, &entry)
            .map_err(|e| self.backend(e))
    }

    fn increment(&self, group: &str, key: &str, by: i64) -> Result<Option<i64>, CacheError> {
        self.adjust(group, key, by, None)
    }

    /// Floors at 0, as memcached does, in the same atomic call.
    fn decrement(&self, group: &str, key: &str, by: i64) -> Result<Option<i64>, CacheError> {
        self.adjust(group, key, by.saturating_neg(), Some(0))
    }

    fn flush(&self) -> Result<(), CacheError> {
        self.store
            .clear(&self.keyspace)
            .map_err(|e| self.backend(e))
    }

    /// Bumps the group's generation: old entries are no longer addressed and go by their own expiry or at `flush`.
    fn flush_group(&self, group: &str) -> Result<(), CacheError> {
        let key = keys::generation(group.as_bytes());
        let generation = self
            .store
            .increment(&self.keyspace, &key, 1)
            .map_err(|e| self.backend(e))?;
        // Where the store keeps keys in order, the older generations' entries are one contiguous range: remove
        // them, so a flushed group does not stay on disk. Best effort: the flush itself has already happened.
        if self.store.capabilities().ordered_ranges {
            let _ = self.purge_generations(group, u64::try_from(generation).unwrap_or(0));
        }
        Ok(())
    }
}
