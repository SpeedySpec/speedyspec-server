//! Conformance suites for speedyspec's data stores. Each engine's tests call the suite for its store kind; a
//! failing check panics with its name.

mod direct;
mod nosql;
mod sql;

pub use direct::direct_store_suite;
pub use nosql::nosql_store_suite;
pub use sql::sql_store_suite;
