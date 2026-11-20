//! The SQLite engine: MySQL statements translated for SQLite, answered with MySQL's results, collation and
//! functions.

mod collation;
mod connection;
mod functions;

pub use collation::fold;
pub use connection::{SqliteTarget, open_connection};
pub use functions::register;
