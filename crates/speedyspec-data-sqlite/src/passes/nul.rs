//! `SqliteNul`: a string literal holding a NUL byte as SQLite can read it.
//!
//! MySQL reads mysqli's `\0` escape inside a string literal as a NUL byte, and the parser keeps it so. Written back
//! into SQL text that NUL is a raw byte, which SQLite's tokenizer rejects ("unrecognized token"). The literal becomes
//! `CAST(X'..' AS TEXT)` instead: the same bytes as a blob literal, read as text, which SQLite stores with the NUL
//! and its length intact.

use std::ops::ControlFlow;

use speedyspec_data::{DbError, Statement};
use speedyspec_data_query::{PassContext, Rewrite, RewritePass};
use sqlparser::ast::{CastKind, DataType, Expr, Value, VisitMut, VisitorMut};

pub struct SqliteNul;

struct Nul;

/// The bytes as upper-case hex digits, as `X'..'` takes them.
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02X}")).collect()
}

impl VisitorMut for Nul {
    type Break = ();

    fn post_visit_expr(&mut self, expr: &mut Expr) -> ControlFlow<()> {
        if let Expr::Value(value) = expr
            && let Value::SingleQuotedString(text) = &value.value
            && text.contains('\0')
        {
            *expr = Expr::Cast {
                kind: CastKind::Cast,
                expr: Box::new(Expr::value(Value::HexStringLiteral(hex(text.as_bytes())))),
                data_type: DataType::Text,
                format: None,
            };
        }
        ControlFlow::Continue(())
    }
}

impl RewritePass for SqliteNul {
    fn name(&self) -> &'static str {
        "sqlite_nul"
    }

    fn rewrite(&self, stmt: &mut Statement, _cx: &PassContext) -> Result<Rewrite, DbError> {
        let _ = stmt.tree_mut().visit(&mut Nul);
        Ok(Rewrite::Continue)
    }
}
