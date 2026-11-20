//! Result column names, pinned before any pass changes the expressions: MySQL names an unaliased expression by its
//! text, PostgreSQL by its function (`count`), so each gets the alias MySQL's name.

use std::ops::ControlFlow;

use speedyspec_data::Statement;
use sqlparser::ast::{Expr, Ident, Select, SelectItem, Value, VisitMut, VisitorMut};

struct Pin;

impl VisitorMut for Pin {
    type Break = ();

    fn pre_visit_select(&mut self, select: &mut Select) -> ControlFlow<()> {
        for item in &mut select.projection {
            let SelectItem::UnnamedExpr(expr) = item else {
                continue;
            };
            let name = match &*expr {
                Expr::Identifier(_) | Expr::CompoundIdentifier(_) => continue,
                Expr::Value(v) => match &v.value {
                    Value::SingleQuotedString(s) | Value::DoubleQuotedString(s) => s.clone(),
                    other => other.to_string(),
                },
                other => other.to_string(),
            };
            let name = if name.is_empty() {
                crate::plan::EMPTY_NAME.to_string()
            } else {
                name
            };
            let expr = std::mem::replace(expr, Expr::value(Value::Null));
            *item = SelectItem::ExprWithAlias {
                expr,
                alias: Ident::with_quote('"', name),
            };
        }
        ControlFlow::Continue(())
    }
}

/// Gives every unaliased, non-column projection item MySQL's name for it.
pub(crate) fn pin(stmt: &mut Statement) {
    let _ = stmt.tree_mut().visit(&mut Pin);
}
