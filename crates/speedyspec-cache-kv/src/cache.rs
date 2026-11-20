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

    /// Adds `by` to a live entry; a value the store cannot count (not canonical, or overflowing) is counted here.
    fn adjust(
        &self,
        group: &str,
        key: &str,
        by: i64,
    ) -> Result<Option<(Vec<u8>, i64)>, CacheError> {
        let entry = self.entry(group, key)?;
        let Some(current) = self
            .store
            .get(&self.keyspace, &entry)
            .map_err(|e| self.backend(e))?
        else {
            return Ok(None);
        };
        match self.store.increment(&self.keyspace, &entry, by) {
            Ok(n) => Ok(Some((entry, n))),
            Err(e) if matches!(engine_code(&e), Some("not-integer" | "overflow")) => {
                let n = number(group, key, &current)?.saturating_add(by);
                // The store cannot report the entry's expiry, so a value it could not count is written without one.
                self.store
                    .put(&self.keyspace, &entry, n.to_string().as_bytes(), None)
                    .map_err(|e| self.backend(e))?;
                Ok(Some((entry, n)))
            }
            Err(e) => Err(self.backend(e)),
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
        Ok(self.adjust(group, key, by)?.map(|(_, n)| n))
    }

    /// Floors at 0: a result below zero is raised back by the difference, which keeps the entry's expiry.
    fn decrement(&self, group: &str, key: &str, by: i64) -> Result<Option<i64>, CacheError> {
        let Some((entry, n)) = self.adjust(group, key, by.saturating_neg())? else {
            return Ok(None);
        };
        if n >= 0 {
            return Ok(Some(n));
        }
        match self
            .store
            .increment(&self.keyspace, &entry, n.saturating_neg())
        {
            Ok(back) => Ok(Some(back.max(0))),
            Err(e) => Err(self.backend(e)),
        }
    }

    fn flush(&self) -> Result<(), CacheError> {
        self.store
            .clear(&self.keyspace)
            .map_err(|e| self.backend(e))
    }

    /// Bumps the group's generation: old entries are no longer addressed and go by their own expiry or at `flush`.
    fn flush_group(&self, group: &str) -> Result<(), CacheError> {
        let key = keys::generation(group.as_bytes());
        self.store
            .increment(&self.keyspace, &key, 1)
            .map(|_| ())
            .map_err(|e| self.backend(e))
    }
}
