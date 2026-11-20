//! Conformance suites for speedyspec's data stores. Each engine's tests call the suite for its store kind; a
//! failing check panics with its name.

mod sql;

pub use sql::sql_store_suite;
