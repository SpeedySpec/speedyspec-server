//! `RowLimitPass`: a single-table `UPDATE` or `DELETE` with `ORDER BY` or `LIMIT`, which neither SQLite (built
//! without the option) nor PostgreSQL accepts, as a row-id subquery.

use speedyspec_data::{DbError, Statement};
use speedyspec_data_query::{PassContext, Rewrite, RewritePass, parse_one};
use sqlparser::ast::{
    Expr, FromTable, Ident, InTarget, LimitClause, OrderBy, OrderByKind, Query, SetExpr,
    Statement as Ast, TableFactor, TableWithJoins,
};

/// `… WHERE <row_id> IN (SELECT <row_id> FROM t WHERE <where> ORDER BY … LIMIT n)`; `row_id` is the engine's own
/// (`rowid` for SQLite, `ctid` for PostgreSQL).
pub struct RowLimitPass {
    row_id: &'static str,
}

impl RowLimitPass {
    pub fn new(row_id: &'static str) -> RowLimitPass {
        RowLimitPass { row_id }
    }

    /// `SELECT <row_id> FROM <table> [WHERE …] [ORDER BY …] [LIMIT n]`, from a skeleton this pass parses itself.
    fn subquery(
        &self,
        table: &TableWithJoins,
        selection: Option<Expr>,
        order_by: Vec<sqlparser::ast::OrderByExpr>,
        limit: Option<Expr>,
    ) -> Result<Query, DbError> {
        let (tree, _) = parse_one(&format!("SELECT {} FROM t", self.row_id))?.into_parts();
        let Ast::Query(mut query) = tree else {
            unreachable!("a SELECT parses as a query")
        };
        if let SetExpr::Select(select) = &mut *query.body {
            select.from = vec![table.clone()];
            select.selection = selection;
        }
        if !order_by.is_empty() {
            query.order_by = Some(OrderBy {
                kind: OrderByKind::Expressions(order_by),
                interpolate: None,
            });
        }
        query.limit_clause = limit.map(|limit| LimitClause::LimitOffset {
            limit: Some(limit),
            offset: None,
            limit_by: Vec::new(),
        });
        Ok(*query)
    }

    fn in_subquery(&self, query: Query) -> Expr {
        Expr::In {
            expr: Box::new(Expr::Identifier(Ident::new(self.row_id))),
            target: InTarget::Subquery(Box::new(query)),
            negated: false,
            global: false,
        }
    }
}

fn single(table: &TableWithJoins) -> bool {
    table.joins.is_empty() && matches!(table.relation, TableFactor::Table { .. })
}

impl RewritePass for RowLimitPass {
    fn name(&self) -> &'static str {
        "row_limit"
    }

    fn rewrite(&self, stmt: &mut Statement, _cx: &PassContext) -> Result<Rewrite, DbError> {
        match stmt.tree_mut() {
            Ast::Delete(delete) if !delete.order_by.is_empty() || delete.limit.is_some() => {
                let tables = match &delete.from {
                    FromTable::WithFromKeyword(t) | FromTable::WithoutKeyword(t) => t.clone(),
                };
                if !(delete.tables.is_empty()
                    && delete.using.is_none()
                    && tables.len() == 1
                    && single(&tables[0]))
                {
                    return Ok(Rewrite::Continue);
                }
                let query = self.subquery(
                    &tables[0],
                    delete.selection.take(),
                    std::mem::take(&mut delete.order_by),
                    delete.limit.take(),
                )?;
                delete.selection = Some(self.in_subquery(query));
            }
            Ast::Update(update) if !update.order_by.is_empty() || update.limit.is_some() => {
                if update.from.is_some() || !single(&update.table) {
                    return Ok(Rewrite::Continue);
                }
                let query = self.subquery(
                    &update.table,
                    update.selection.take(),
                    std::mem::take(&mut update.order_by),
                    update.limit.take(),
                )?;
                update.selection = Some(self.in_subquery(query));
            }
            _ => {}
        }
        Ok(Rewrite::Continue)
    }
}
