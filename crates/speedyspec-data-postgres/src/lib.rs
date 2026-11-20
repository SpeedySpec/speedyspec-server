//! The PostgreSQL engine: WordPress's MySQL statements translated for PostgreSQL, answered with MySQL's results.

mod dialect;
mod passes;
mod plan;
mod result_names;
mod schema;
mod types;

pub use dialect::PostgresDialect;
pub use plan::{
    COUNT_ALIAS, ID_ALIAS, INSERTED_ALIAS, PgEmitted, PgPlan, RowCount, SEQUENCE_ALIAS,
};
pub use types::mysql_display;
