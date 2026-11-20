//! MySQL's dialect (Strategy): MySQL is the source dialect, so statements are emitted as parsed.

use speedyspec_data::{DbError, Dialect, DialectKind, EmittedSql, Prepared, Statement};
use speedyspec_data_query::{PassChain, PassContext, Rewritten, mysql_text};

/// Emits a statement's tree as MySQL text with `?` placeholders; never emulates.
#[derive(Debug, Clone, Copy, Default)]
pub struct MySqlDialect;

impl Dialect for MySqlDialect {
    fn kind(&self) -> DialectKind {
        DialectKind::MySql
    }

    fn prepare(&self, stmt: &Statement) -> Result<Prepared, DbError> {
        let cx = PassContext {
            target: DialectKind::MySql,
            introspector: None,
        };
        match PassChain::new().run(stmt.clone(), &cx)? {
            Rewritten::Statements(stmts) => Ok(Prepared::Sql(
                stmts
                    .into_iter()
                    .map(|s| EmittedSql {
                        sql: mysql_text(s.tree()),
                        params: s.params().to_vec(),
                    })
                    .collect(),
            )),
            Rewritten::Emulated(op) => Err(DbError::Unsupported {
                construct: format!("emulated statement {op:?}"),
                dialect: DialectKind::MySql,
            }),
        }
    }
}
