//! In-memory NoSQL and Direct Stores (Adapters over a map), for tests and single-process sites.

mod direct;
mod factory;
mod nosql;

pub use direct::MemoryDirectStore;
pub use factory::MemoryEngineFactory;
pub use nosql::MemoryNoSqlStore;
