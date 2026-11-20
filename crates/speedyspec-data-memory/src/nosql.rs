//! `MemoryNoSqlStore`: keyspaces of ordered byte keys, each value with an optional expiry.

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::ops::Bound;
use std::time::{Duration, Instant};

use speedyspec_data::{Capabilities, DbError, KeyValues, NoSqlStore, UpsertForm};
use speedyspec_data_counter::checked_increment;

type Entry = (Vec<u8>, Option<Instant>);

pub struct MemoryNoSqlStore {
    engine: String,
    keyspaces: RefCell<HashMap<String, BTreeMap<Vec<u8>, Entry>>>,
}

/// When a TTL ends; under a millisecond lives a millisecond, as on Valkey; one past the clock's range never ends.
fn deadline(ttl: Duration) -> Option<Instant> {
    Instant::now().checked_add(ttl.max(Duration::from_millis(1)))
}

fn live(entry: &Entry) -> bool {
    entry.1.is_none_or(|at| Instant::now() < at)
}

impl MemoryNoSqlStore {
    pub fn new(engine: &str) -> MemoryNoSqlStore {
        MemoryNoSqlStore {
            engine: engine.to_string(),
            keyspaces: RefCell::new(HashMap::new()),
        }
    }

    /// The live entry, dropping it when it has expired.
    fn entry(&self, keyspace: &str, key: &[u8]) -> Option<Entry> {
        let mut spaces = self.keyspaces.borrow_mut();
        let space = spaces.get_mut(keyspace)?;
        match space.get(key) {
            Some(e) if live(e) => Some(e.clone()),
            Some(_) => {
                space.remove(key);
                None
            }
            None => None,
        }
    }
}

impl NoSqlStore for MemoryNoSqlStore {
    fn engine(&self) -> &str {
        &self.engine
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            transactions: false,
            savepoints: false,
            ordered_ranges: true,
            expiry: true,
            atomic_increment: true,
            returning: false,
            upsert: UpsertForm::None,
        }
    }

    fn get(&self, keyspace: &str, key: &[u8]) -> Result<Option<Vec<u8>>, DbError> {
        Ok(self.entry(keyspace, key).map(|(v, _)| v))
    }

    fn get_many(&self, keyspace: &str, keys: &[&[u8]]) -> Result<Vec<Option<Vec<u8>>>, DbError> {
        Ok(keys
            .iter()
            .map(|k| self.entry(keyspace, k).map(|(v, _)| v))
            .collect())
    }

    fn put(
        &self,
        keyspace: &str,
        key: &[u8],
        value: &[u8],
        ttl: Option<Duration>,
    ) -> Result<(), DbError> {
        let mut spaces = self.keyspaces.borrow_mut();
        let space = spaces.entry(keyspace.to_string()).or_default();
        match ttl {
            // A zero TTL deletes, as Valkey's SET with an expiry already past does.
            Some(t) if t.is_zero() => {
                space.remove(key);
            }
            _ => {
                space.insert(key.to_vec(), (value.to_vec(), ttl.and_then(deadline)));
            }
        }
        Ok(())
    }

    fn put_if_absent(
        &self,
        keyspace: &str,
        key: &[u8],
        value: &[u8],
        ttl: Option<Duration>,
    ) -> Result<bool, DbError> {
        if self.entry(keyspace, key).is_some() {
            return Ok(false);
        }
        self.put(keyspace, key, value, ttl)?;
        Ok(!matches!(ttl, Some(t) if t.is_zero()))
    }

    fn delete(&self, keyspace: &str, key: &[u8]) -> Result<bool, DbError> {
        let found = self.entry(keyspace, key).is_some();
        if let Some(space) = self.keyspaces.borrow_mut().get_mut(keyspace) {
            space.remove(key);
        }
        Ok(found)
    }

    fn range(
        &self,
        keyspace: &str,
        start: Bound<&[u8]>,
        end: Bound<&[u8]>,
        limit: Option<usize>,
    ) -> Result<KeyValues, DbError> {
        let spaces = self.keyspaces.borrow();
        let Some(space) = spaces.get(keyspace) else {
            return Ok(Vec::new());
        };
        let range = (start.map(<[u8]>::to_vec), end.map(<[u8]>::to_vec));
        Ok(space
            .range::<Vec<u8>, _>(range)
            .filter(|(_, e)| live(e))
            .take(limit.unwrap_or(usize::MAX))
            .map(|(k, (v, _))| (k.clone(), v.clone()))
            .collect())
    }

    fn increment(&self, keyspace: &str, key: &[u8], by: i64) -> Result<i64, DbError> {
        let current = self.entry(keyspace, key);
        let (next, bytes) = checked_increment(
            &self.engine,
            current.as_ref().map(|(v, _)| v.as_slice()),
            by,
        )?;
        // The increment keeps the key's expiry.
        let expires = current.and_then(|(_, at)| at);
        self.keyspaces
            .borrow_mut()
            .entry(keyspace.to_string())
            .or_default()
            .insert(key.to_vec(), (bytes, expires));
        Ok(next)
    }

    fn clear(&self, keyspace: &str) -> Result<(), DbError> {
        self.keyspaces.borrow_mut().remove(keyspace);
        Ok(())
    }
}
