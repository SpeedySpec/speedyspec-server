//! `MemorySitemetaRepository`: `wp_sitemeta` in a list, in insertion order, answering as MySQL's table does.

use std::cell::RefCell;

use speedyspec_data::DbError;
use speedyspec_wp_options::{NetworkMeta, SitemetaRepository};

#[derive(Default)]
pub struct MemorySitemetaRepository {
    rows: RefCell<Vec<NetworkMeta>>,
}

impl MemorySitemetaRepository {
    pub fn new() -> MemorySitemetaRepository {
        MemorySitemetaRepository::default()
    }
}

fn matches(row: &NetworkMeta, network_id: i64, key: &str) -> bool {
    row.network_id == network_id && row.key == key
}

impl SitemetaRepository for MemorySitemetaRepository {
    fn find(&self, network_id: i64, key: &str) -> Result<Option<NetworkMeta>, DbError> {
        Ok(self
            .rows
            .borrow()
            .iter()
            .find(|r| matches(r, network_id, key))
            .cloned())
    }

    fn find_many(&self, network_id: i64, keys: &[String]) -> Result<Vec<NetworkMeta>, DbError> {
        Ok(self
            .rows
            .borrow()
            .iter()
            .filter(|r| r.network_id == network_id && keys.contains(&r.key))
            .cloned()
            .collect())
    }

    fn exists(&self, network_id: i64, key: &str) -> Result<bool, DbError> {
        Ok(self
            .rows
            .borrow()
            .iter()
            .any(|r| matches(r, network_id, key)))
    }

    /// `wp_sitemeta` has no unique key: a second row for the same key is a second row, as MySQL keeps it.
    fn add(&self, meta: NetworkMeta) -> Result<bool, DbError> {
        self.rows.borrow_mut().push(meta);
        Ok(true)
    }

    fn update(&self, meta: NetworkMeta) -> Result<u64, DbError> {
        let mut changed = 0;
        for row in self.rows.borrow_mut().iter_mut() {
            if matches(row, meta.network_id, &meta.key) && row.value != meta.value {
                row.value = meta.value.clone();
                changed += 1;
            }
        }
        Ok(changed)
    }

    fn remove(&self, network_id: i64, key: &str) -> Result<u64, DbError> {
        let mut rows = self.rows.borrow_mut();
        let before = rows.len();
        rows.retain(|r| !matches(r, network_id, key));
        Ok((before - rows.len()) as u64)
    }
}
