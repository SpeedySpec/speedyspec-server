//! The PostgreSQL dialect (Strategy): a MySQL statement as the statements PostgreSQL runs, each with how it counts.

use std::rc::Rc;

use speedyspec_data::{DbError, Dialect, DialectKind, Introspector, Prepared, Statement};
use speedyspec_data_query::{PassChain, PassContext, Rewritten};
use speedyspec_data_translate::{number_placeholders, take_found_rows};

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

    /// Whether reading the schema failed while planning since the last call; the plan may then lack what the schema
    /// would have added (an insert's RETURNING, a guard).
    pub(crate) fn take_schema_failure(&self) -> bool {
        self.schema.take_failure()
    }

    pub fn plan(&self, stmt: &Statement) -> Result<PgPlan, DbError> {
        let mut stmt = stmt.clone();
        number_placeholders(&mut stmt);
        let count = take_found_rows(&mut stmt);
        result_names::pin(&mut stmt);
        let cx = PassContext {
            target: DialectKind::Postgres,
            introspector: self.schema.introspector(),
        };
        let emit = |statements: Vec<Statement>| -> Vec<crate::plan::PgEmitted> {
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
                .collect()
        };
        let mut out = match self.chain.run(stmt, &cx)? {
            Rewritten::Emulated(op) => return Ok(PgPlan::Emulated(op)),
            Rewritten::Statements(statements) => emit(statements),
        };
        if let Some(count) = count
            && let Rewritten::Statements(statements) = self.chain.run(count, &cx)?
        {
            out.extend(emit(statements).into_iter().map(|mut e| {
                e.count = crate::plan::RowCount::FoundRows;
                e
            }));
        }
        Ok(PgPlan::Sql(out))
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
