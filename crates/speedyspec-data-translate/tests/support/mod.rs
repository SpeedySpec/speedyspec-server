//! Shared by the translate tests: run one pass, and compare a tree with the SQL it should print as.

#![allow(dead_code)]

use speedyspec_data::{DbError, DialectKind, Statement};
use speedyspec_data_query::{PassChain, PassContext, RewritePass, Rewritten, parse_one};
use sqlparser::dialect::GenericDialect;
use sqlparser::parser::Parser;

pub fn cx() -> PassContext<'static> {
    PassContext {
        target: DialectKind::Sqlite,
        introspector: None,
    }
}

/// Parses `sql` (MySQL text) and runs `pass` alone over it.
pub fn rewrite_one(
    pass: impl RewritePass + 'static,
    sql: &str,
    cx: &PassContext,
) -> Result<Rewritten, DbError> {
    let mut chain = PassChain::new();
    chain.push(Box::new(pass));
    chain.run(parse_one(sql)?, cx)
}

/// The one statement a rewrite left.
pub fn statement_of(rewritten: Rewritten) -> Statement {
    match rewritten {
        Rewritten::Statements(mut stmts) if stmts.len() == 1 => stmts.remove(0),
        other => panic!("expected one statement, got {other:?}"),
    }
}

/// `stmt` prints as `expected` does once `sqlparser`'s generic dialect has parsed and printed it again.
pub fn assert_sql(stmt: &Statement, expected: &str) {
    let parsed = Parser::parse_sql(&GenericDialect {}, expected)
        .unwrap_or_else(|e| panic!("{expected}: {e}"));
    assert_eq!(stmt.tree().to_string(), parsed[0].to_string());
}
