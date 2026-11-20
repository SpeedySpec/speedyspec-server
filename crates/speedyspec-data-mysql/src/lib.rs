//! The MySQL engine for speedyspec's data layer (Abstract Factory: store, dialect, introspector).
//!
//! `MySqlStore` adapts the blocking `mysql` driver (Adapter); `MySqlDialect` emits statements as parsed (Strategy).
//! With params a statement uses the binary protocol, without them the text protocol. The connection never sets
//! `CLIENT_FOUND_ROWS`, so `rows_affected` counts changed rows, as mysqli does.

mod dialect;
mod factory;
mod introspect;
mod store;

pub use dialect::MySqlDialect;
pub use factory::MySqlEngineFactory;
pub use store::MySqlStore;
