//! The SQLite engine: MySQL statements translated for SQLite, answered with MySQL's results, collation and
//! functions.

mod collation;
mod connection;
mod dialect;
mod errors;
mod factory;
mod functions;
mod info_schema;
mod introspect;
mod passes;
mod plan;
mod results;
mod store;

pub use collation::fold;
pub use connection::{SqliteTarget, open_connection};
pub use dialect::SqliteDialect;
pub use errors::map_error;
pub use factory::{SqliteEngine, SqliteEngineFactory};
pub use functions::register;
pub use passes::{SqliteAlter, SqliteQuote, SqliteTypes, sqlite_chain};
pub use plan::{CountRule, InsertIdRule, SqlitePlan, SqliteStep};
pub use store::SqliteStore;
