use std::fmt;

use crate::{Fingerprint, RouteData};

/// Why a store could not answer. Callers treat it as a miss: the store is only a cache.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreError(pub String);

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "route table store: {}", self.0)
    }
}

impl std::error::Error for StoreError {}

/// Caches route data by fingerprint, as FastRoute caches its generated data.
///
/// Every worker can rebuild the data from its own inputs, so any implementation, shared or not, keeps replicas free
/// of coordination. The default is in memory; one may be supplied from PHP.
pub trait RouteTableStore {
    fn get(&self, fingerprint: &Fingerprint) -> Result<Option<RouteData>, StoreError>;
    fn put(&self, fingerprint: &Fingerprint, data: &RouteData) -> Result<(), StoreError>;
    fn forget(&self, fingerprint: &Fingerprint) -> Result<(), StoreError>;
}
