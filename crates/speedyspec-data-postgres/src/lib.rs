//! The PostgreSQL engine: WordPress's MySQL statements translated for PostgreSQL, answered with MySQL's results.

mod dialect;
mod passes;
mod plan;
mod result_names;
mod schema;
mod types;
mod value;

pub use dialect::PostgresDialect;
pub use plan::{
    COUNT_ALIAS, ID_ALIAS, INSERTED_ALIAS, PgEmitted, PgPlan, RowCount, SEQUENCE_ALIAS,
};
pub use types::mysql_display;

/// The crate's value adapters, for its own tests.
#[doc(hidden)]
pub mod __test {
    pub use crate::value::{PgCell, PgParam, decode};
}
