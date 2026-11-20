//! The PostgreSQL dialect (Strategy): a MySQL statement as the statements PostgreSQL runs, each with how it counts.

use std::rc::Rc;

use speedyspec_data::{DbError, Dialect, DialectKind, Introspector, Prepared, Statement};
use speedyspec_data_query::{PassChain, PassContext, Rewritten};
use speedyspec_data_translate::number_placeholders;

use crate::passes::pg_chain;
use crate::plan::{PgPlan, emitted};
use crate::result_names;
use crate::schema::SchemaCache;

pub struct PostgresDialect {
    schema: Rc<SchemaCache>,
    chain: PassChain,
}

impl PostgresDialect {
    /// A dialect reading the schema through `introspector` (an open engine's; `None` plans without it).
    pub fn new(introspector: Option<Rc<dyn Introspector>>) -> PostgresDialect {
        let schema = Rc::new(SchemaCache::new(introspector));
        PostgresDialect {
            chain: pg_chain(&schema),
            schema,
        }
    }

    /// Forgets what the passes read of the schema, after DDL.
    pub fn invalidate_schema(&self) {
        self.schema.clear();
    }

    pub fn plan(&self, stmt: &Statement) -> Result<PgPlan, DbError> {
        let mut stmt = stmt.clone();
        number_placeholders(&mut stmt);
        result_names::pin(&mut stmt);
        let cx = PassContext {
            target: DialectKind::Postgres,
            introspector: self.schema.introspector(),
        };
        match self.chain.run(stmt, &cx)? {
            Rewritten::Emulated(op) => Ok(PgPlan::Emulated(op)),
            Rewritten::Statements(statements) => Ok(PgPlan::Sql(
                statements
                    .into_iter()
                    .map(|s| {
                        let (tree, params) = s.into_parts();
                        emitted(
                            speedyspec_data::EmittedSql {
                                sql: tree.to_string(),
                                params,
                            },
                            &tree,
                        )
                    })
                    .collect(),
            )),
        }
    }
}

impl Dialect for PostgresDialect {
    fn kind(&self) -> DialectKind {
        DialectKind::Postgres
    }

    fn prepare(&self, stmt: &Statement) -> Result<Prepared, DbError> {
        Ok(match self.plan(stmt)? {
            PgPlan::Emulated(op) => Prepared::Emulated(op),
            PgPlan::Sql(statements) => {
                Prepared::Sql(statements.into_iter().map(|e| e.sql).collect())
            }
        })
    }
}
