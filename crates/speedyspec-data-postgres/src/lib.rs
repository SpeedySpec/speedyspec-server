//! The PostgreSQL engine: WordPress's MySQL statements translated for PostgreSQL, answered with MySQL's results.

mod connection;
mod dialect;
mod error;
mod exec;
mod factory;
mod introspect;
mod passes;
mod plan;
mod result_names;
mod schema;
mod store;
mod support;
mod types;
mod value;

pub use dialect::PostgresDialect;
pub use error::map_error;
pub use factory::{PostgresEngine, PostgresEngineFactory};
pub use introspect::PostgresIntrospector;
pub use plan::{
    COUNT_ALIAS, ID_ALIAS, INSERTED_ALIAS, PgEmitted, PgPlan, RowCount, SEQUENCE_ALIAS,
};
pub use store::PostgresStore;
pub use support::SUPPORT_SQL;
pub use types::mysql_display;

/// The crate's value adapters, for its own tests.
#[doc(hidden)]
pub mod __test {
    pub use crate::connection::{Connection, check_server_version};
    pub use crate::value::{PgCell, PgParam, decode};

    /// An introspector over a connection, for the live tests.
    pub fn introspector(
        conn: std::rc::Rc<std::cell::RefCell<Connection>>,
    ) -> crate::PostgresIntrospector {
        crate::PostgresIntrospector::new(conn)
    }

    /// Runs one emitted statement for the live tests.
    pub fn execute(
        conn: &mut Connection,
        sql: &speedyspec_data::EmittedSql,
    ) -> Result<u64, speedyspec_data::DbError> {
        let engine = conn.engine().to_string();
        crate::exec::execute(conn, &engine, sql)
    }
}
