//! A Caching Store in process memory (Strategy): the object cache's stand-in second level in tests, and a cache for
//! one worker's life.

use std::cell::RefCell;
use std::collections::HashMap;
use std::time::{Duration, Instant};

use speedyspec_cache::{CacheError, CachingStore};

type Entry = (Vec<u8>, Option<Instant>);

/// Entries by group and key; an expired entry is dropped when met.
#[derive(Default)]
pub struct MemoryCache {
    groups: RefCell<HashMap<String, HashMap<String, Entry>>>,
}

fn deadline(expiry: Option<Duration>) -> Option<Instant> {
    // An expiry past what the clock can hold never comes.
    expiry.and_then(|d| Instant::now().checked_add(d))
}

fn live(entry: &Entry) -> bool {
    entry.1.is_none_or(|at| Instant::now() < at)
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

impl MemoryCache {
    pub fn new() -> MemoryCache {
        MemoryCache::default()
    }

    /// The live entry, dropping it when expired.
    fn with_live<T>(&self, group: &str, key: &str, f: impl FnOnce(Option<&mut Entry>) -> T) -> T {
        let mut groups = self.groups.borrow_mut();
        let entries = groups.entry(group.to_string()).or_default();
        if entries.get(key).is_some_and(|e| !live(e)) {
            entries.remove(key);
        }
        f(entries.get_mut(key))
    }

    fn put(&self, group: &str, key: &str, value: &[u8], expiry: Option<Duration>) {
        self.groups
            .borrow_mut()
            .entry(group.to_string())
            .or_default()
            .insert(key.to_string(), (value.to_vec(), deadline(expiry)));
    }

    fn adjust(&self, group: &str, key: &str, by: i64) -> Result<Option<i64>, CacheError> {
        self.with_live(group, key, |entry| match entry {
            None => Ok(None),
            Some(entry) => {
                let n = number(group, key, &entry.0)?.saturating_add(by);
                entry.0 = n.to_string().into_bytes();
                Ok(Some(n))
            }
        })
    }
}

impl CachingStore for MemoryCache {
    fn name(&self) -> &str {
        "memory"
    }

    fn get(&self, group: &str, key: &str) -> Result<Option<Vec<u8>>, CacheError> {
        Ok(self.with_live(group, key, |e| e.map(|e| e.0.clone())))
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
        self.put(group, key, value, expiry);
        Ok(())
    }

    fn add(
        &self,
        group: &str,
        key: &str,
        value: &[u8],
        expiry: Option<Duration>,
    ) -> Result<bool, CacheError> {
        if self.with_live(group, key, |e| e.is_some()) {
            return Ok(false);
        }
        self.put(group, key, value, expiry);
        Ok(true)
    }

    fn replace(
        &self,
        group: &str,
        key: &str,
        value: &[u8],
        expiry: Option<Duration>,
    ) -> Result<bool, CacheError> {
        if !self.with_live(group, key, |e| e.is_some()) {
            return Ok(false);
        }
        self.put(group, key, value, expiry);
        Ok(true)
    }

    fn delete(&self, group: &str, key: &str) -> Result<bool, CacheError> {
        let present = self.with_live(group, key, |e| e.is_some());
        if present && let Some(entries) = self.groups.borrow_mut().get_mut(group) {
            entries.remove(key);
        }
        Ok(present)
    }

    fn increment(&self, group: &str, key: &str, by: i64) -> Result<Option<i64>, CacheError> {
        self.adjust(group, key, by)
    }

    fn decrement(&self, group: &str, key: &str, by: i64) -> Result<Option<i64>, CacheError> {
        self.with_live(group, key, |entry| match entry {
            None => Ok(None),
            Some(entry) => {
                let n = number(group, key, &entry.0)?.saturating_sub(by).max(0);
                entry.0 = n.to_string().into_bytes();
                Ok(Some(n))
            }
        })
    }

    fn flush(&self) -> Result<(), CacheError> {
        self.groups.borrow_mut().clear();
        Ok(())
    }

    fn flush_group(&self, group: &str) -> Result<(), CacheError> {
        self.groups.borrow_mut().remove(group);
        Ok(())
    }
}
