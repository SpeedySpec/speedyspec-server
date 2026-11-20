//! The Caching Store contract: byte values under a group and a key, with optional expiry.
//!
//! Implementations (Strategy) live in their own crates: memory, SQL, key-value and file stores.

mod error;

use std::time::Duration;

pub use error::CacheError;

/// A cache of opaque byte values, keyed by group and key.
///
/// - Values are opaque bytes; the empty value is a value, not a miss.
/// - `None` expiry never expires; `Some(Duration::ZERO)` is expired at once. An expired entry behaves as absent
///   everywhere (`get`, `add`, `replace`, `delete`, `increment`).
/// - `increment` and `decrement` read the value as an ASCII signed decimal integer (`NotANumber` otherwise), store
///   the result the same way, keep the entry's expiry, and return `None` when the entry is absent; `decrement`
///   floors at 0, as memcached does.
/// - `flush_group` touches only that group.
pub trait CachingStore {
    fn name(&self) -> &str;
    fn get(&self, group: &str, key: &str) -> Result<Option<Vec<u8>>, CacheError>;
    /// One entry per key, in order, duplicates included.
    fn get_many(&self, group: &str, keys: &[&str]) -> Result<Vec<Option<Vec<u8>>>, CacheError>;
    fn set(
        &self,
        group: &str,
        key: &str,
        value: &[u8],
        expiry: Option<Duration>,
    ) -> Result<(), CacheError>;
    /// False when a live entry exists.
    fn add(
        &self,
        group: &str,
        key: &str,
        value: &[u8],
        expiry: Option<Duration>,
    ) -> Result<bool, CacheError>;
    /// False when no live entry exists.
    fn replace(
        &self,
        group: &str,
        key: &str,
        value: &[u8],
        expiry: Option<Duration>,
    ) -> Result<bool, CacheError>;
    /// True when a live entry was removed.
    fn delete(&self, group: &str, key: &str) -> Result<bool, CacheError>;
    fn increment(&self, group: &str, key: &str, by: i64) -> Result<Option<i64>, CacheError>;
    fn decrement(&self, group: &str, key: &str, by: i64) -> Result<Option<i64>, CacheError>;
    fn flush(&self) -> Result<(), CacheError>;
    fn flush_group(&self, group: &str) -> Result<(), CacheError>;
}
