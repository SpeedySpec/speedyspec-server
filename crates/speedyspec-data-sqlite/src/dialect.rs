//! The SQLite dialect (Strategy): a MySQL statement as the steps SQLite runs.

use speedyspec_data::{
    DbError, Dialect, DialectKind, EmittedSql, Introspector, Prepared, Statement,
};
use speedyspec_data_query::{PassChain, PassContext, Rewritten};
use speedyspec_data_translate::{number_placeholders, take_found_rows};

use crate::passes::sqlite_chain;
use crate::plan::{SqlitePlan, emit};

pub struct SqliteDialect {
    chain: PassChain,
}

impl Default for SqliteDialect {
    fn default() -> SqliteDialect {
        SqliteDialect::new()
    }
}

impl SqliteDialect {
    pub fn new() -> SqliteDialect {
        SqliteDialect {
            chain: sqlite_chain(),
        }
    }

    fn translate(&self, stmt: Statement, cx: &PassContext) -> Result<Rewritten, DbError> {
        self.chain.run(stmt, cx)
    }

    /// The plan for `stmt`; `introspector` is the open engine's schema, which `ALTER TABLE` needs.
    pub fn plan(
        &self,
        stmt: &Statement,
        introspector: Option<&dyn Introspector>,
    ) -> Result<SqlitePlan, DbError> {
        let cx = PassContext {
            target: DialectKind::Sqlite,
            introspector,
        };
        let mut stmt = stmt.clone();
        number_placeholders(&mut stmt);
        let kind = stmt.kind();
        let count = take_found_rows(&mut stmt);
        let steps = match self.translate(stmt, &cx)? {
            Rewritten::Emulated(op) => return Ok(SqlitePlan::Emulated(op)),
            Rewritten::Statements(statements) => emit(kind, statements),
        };
        let found_rows = match count {
            Some(count) => match self.translate(count, &cx)? {
                Rewritten::Statements(statements) => {
                    emit(speedyspec_data::StatementKind::Select, statements)
                        .pop()
                        .map(|s| s.sql)
                }
                Rewritten::Emulated(_) => None,
            },
            None => None,
        };
        let information_schema = steps.iter().any(|s| {
            s.sql
                .sql
                .to_ascii_lowercase()
                .contains("information_schema")
        });
        Ok(SqlitePlan::Run {
            steps,
            found_rows,
            information_schema,
        })
    }
}

impl Dialect for SqliteDialect {
    fn kind(&self) -> DialectKind {
        DialectKind::Sqlite
    }

    fn prepare(&self, stmt: &Statement) -> Result<Prepared, DbError> {
        match self.plan(stmt, None)? {
            SqlitePlan::Emulated(op) => Ok(Prepared::Emulated(op)),
            SqlitePlan::Run {
                steps, found_rows, ..
            } => {
                let mut sql: Vec<EmittedSql> = steps.into_iter().map(|s| s.sql).collect();
                sql.extend(found_rows);
                Ok(Prepared::Sql(sql))
            }
        }
    }
}
