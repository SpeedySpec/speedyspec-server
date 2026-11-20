//! The Valkey NoSQL Store (Adapter over the blocking `redis` client): a keyspace is a key prefix.

mod factory;
mod store;

pub use factory::ValkeyEngineFactory;
pub use store::{ValkeyStore, driver_url};
