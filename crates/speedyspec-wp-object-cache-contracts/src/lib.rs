//! The object cache seam's contracts: the `wp_cache_*` port other seams call, and its errors.
//!
//! - [`ObjectCache`] is the object cache as other seams use it. `speedyspec-wp-object-cache` implements it.
//! - [`get_with`] lends a cached value to a closure instead of copying it.
//! - [`ObjectCacheError`] is what PHP throws for a key or group it cannot use.

mod api;
mod error;

pub use api::{ObjectCache, get_with};
pub use error::ObjectCacheError;
