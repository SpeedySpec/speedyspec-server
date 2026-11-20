//! The SQLite engine: MySQL statements translated for SQLite, answered with MySQL's results, collation and
//! functions.

mod collation;
mod connection;
mod dialect;
mod functions;
mod passes;
mod plan;

pub use collation::fold;
pub use connection::{SqliteTarget, open_connection};
pub use dialect::SqliteDialect;
pub use functions::register;
pub use passes::{SqliteAlter, SqliteQuote, SqliteTypes, sqlite_chain};
pub use plan::{CountRule, InsertIdRule, SqlitePlan, SqliteStep};
