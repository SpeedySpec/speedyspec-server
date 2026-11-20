//! MySQL's dialect (Strategy): MySQL is the source dialect, so statements are emitted as parsed.

use std::ops::ControlFlow;

use speedyspec_data::ast::{Value, ValueWithSpan, VisitMut, VisitorMut};
use speedyspec_data::{DbError, Dialect, DialectKind, EmittedSql, Prepared, Statement};
use speedyspec_data_query::{PassChain, PassContext, Rewritten};

/// Emits a statement's tree as MySQL text with `?` placeholders; never emulates.
#[derive(Debug, Clone, Copy, Default)]
pub struct MySqlDialect;

/// The parser stores a string literal's value (MySQL's backslash escapes already applied), and the syntax tree's
/// `Display` re-escapes only the quote. MySQL reads backslashes as escapes, so each literal is escaped again before
/// emission; otherwise `'a\\'` would reach the server as `'a\'` and end somewhere else.
struct Reescape;

fn mysql_escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    for c in value.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\0' => out.push_str("\\0"),
            '\u{1a}' => out.push_str("\\Z"),
            other => out.push(other),
        }
    }
    out
}

impl VisitorMut for Reescape {
    type Break = ();

    fn pre_visit_value(&mut self, value: &mut ValueWithSpan) -> ControlFlow<()> {
        match &mut value.value {
            Value::SingleQuotedString(s)
            | Value::DoubleQuotedString(s)
            | Value::NationalStringLiteral(s) => {
                *s = mysql_escape(s);
            }
            _ => {}
        }
        ControlFlow::Continue(())
    }
}

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
                    .map(|s| {
                        let (mut tree, params) = s.into_parts();
                        let _ = tree.visit(&mut Reescape);
                        EmittedSql {
                            sql: tree.to_string(),
                            params,
                        }
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
