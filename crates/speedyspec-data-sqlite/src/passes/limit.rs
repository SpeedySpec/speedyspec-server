//! `SqliteLimit`: SQLite writes `OFFSET` only after a `LIMIT`; an offset with no limit becomes `LIMIT -1 OFFSET n`
//! (SQLite reads a negative limit as none).

use std::ops::ControlFlow;

use speedyspec_data::{DbError, Statement};
use speedyspec_data_query::{PassContext, Rewrite, RewritePass};
use sqlparser::ast::{Expr, LimitClause, Query, Value, VisitMut, VisitorMut};

pub struct SqliteLimit;

struct Limit;

impl VisitorMut for Limit {
    type Break = ();

    fn pre_visit_query(&mut self, query: &mut Query) -> ControlFlow<()> {
        if let Some(LimitClause::LimitOffset {
            limit: limit @ None,
            offset: Some(_),
            ..
        }) = &mut query.limit_clause
        {
            *limit = Some(Expr::value(Value::Number("-1".into(), false)));
        }
        ControlFlow::Continue(())
    }
}

impl RewritePass for SqliteLimit {
    fn name(&self) -> &'static str {
        "sqlite_limit"
    }

    fn rewrite(&self, stmt: &mut Statement, _cx: &PassContext) -> Result<Rewrite, DbError> {
        let _ = stmt.tree_mut().visit(&mut Limit);
        Ok(Rewrite::Continue)
    }
}
