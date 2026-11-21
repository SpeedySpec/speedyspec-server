//! `PgPlaceholders`: the neutral chain's numbered placeholders (`?1`, `?2`) as PostgreSQL's `$1`, `$2`.

use std::ops::ControlFlow;

use speedyspec_data::{DbError, Statement};
use speedyspec_data_query::{PassContext, Rewrite, RewritePass};
use sqlparser::ast::{Value, ValueWithSpan, VisitMut, VisitorMut};

pub(crate) struct PgPlaceholders;

struct Dollars;

impl VisitorMut for Dollars {
    type Break = ();

    fn pre_visit_value(&mut self, value: &mut ValueWithSpan) -> ControlFlow<()> {
        if let Value::Placeholder(p) = &mut value.value
            && let Some(n) = p.strip_prefix('?')
            && !n.is_empty()
        {
            *p = format!("${n}");
        }
        ControlFlow::Continue(())
    }
}

impl RewritePass for PgPlaceholders {
    fn name(&self) -> &'static str {
        "pg_placeholders"
    }

    fn rewrite(&self, stmt: &mut Statement, _cx: &PassContext) -> Result<Rewrite, DbError> {
        let _ = stmt.tree_mut().visit(&mut Dollars);
        Ok(Rewrite::Continue)
    }
}
