//! `KvCache`: a Caching Store (Strategy) over any NoSQL store. `flush_group` bumps the group's generation counter,
//! so it needs no prefix delete and is seen by every process at once.

mod cache;
pub mod keys;

pub use cache::KvCache;
