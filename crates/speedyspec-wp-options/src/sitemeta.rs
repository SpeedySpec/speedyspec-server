//! Network options' storage: `wp_sitemeta` as its own aggregate (Repository). An entry never stores itself.

use speedyspec_data::{DbError, Identified};

use crate::SerializedValue;

/// One `wp_sitemeta` row: a network's option. `meta_id` is the table's, kept by the mapper only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetworkMeta {
    pub network_id: i64,
    pub key: String,
    pub value: SerializedValue,
}

impl Identified for NetworkMeta {
    type Id = (i64, String);

    fn identity(&self) -> (i64, String) {
        (self.network_id, self.key.clone())
    }
}

/// The statements `option.php` sends to `wp_sitemeta`, one per method.
pub trait SitemetaRepository {
    /// The first row's value for the key on the network.
    fn find(&self, network_id: i64, key: &str) -> Result<Option<NetworkMeta>, DbError>;
    /// The rows found for the keys on the network; missing keys are skipped.
    fn find_many(&self, network_id: i64, keys: &[String]) -> Result<Vec<NetworkMeta>, DbError>;
    fn exists(&self, network_id: i64, key: &str) -> Result<bool, DbError>;
    /// Whether the row was inserted.
    fn add(&self, meta: NetworkMeta) -> Result<bool, DbError>;
    /// Rows changed (a row given its own value is not changed).
    fn update(&self, meta: NetworkMeta) -> Result<u64, DbError>;
    /// Rows removed.
    fn remove(&self, network_id: i64, key: &str) -> Result<u64, DbError>;
}
