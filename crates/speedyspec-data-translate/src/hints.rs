//! `HintsPass`: MySQL's optimizer and priority hints, which no other engine reads, removed.

use std::ops::ControlFlow;

use speedyspec_data::{DbError, Statement};
use speedyspec_data_query::{PassContext, Rewrite, RewritePass};
use sqlparser::ast::{
    JoinOperator, Select, SelectModifiers, Statement as Ast, TableFactor, TableWithJoins, VisitMut,
    VisitorMut,
};

/// Drops index hints, `HIGH_PRIORITY`, `LOW_PRIORITY`, `DELAYED`, `SQL_NO_CACHE`, `SQL_CACHE`, `SQL_SMALL_RESULT`,
/// `SQL_BIG_RESULT`, `SQL_BUFFER_RESULT`, and turns `STRAIGHT_JOIN` into `JOIN`. `SQL_CALC_FOUND_ROWS` stays (the
/// found-rows pass owns it).
pub struct HintsPass;

struct Hints;

fn joins(from: &mut [TableWithJoins]) {
    for table in from {
        for join in &mut table.joins {
            if let JoinOperator::StraightJoin(constraint) = &join.join_operator {
                join.join_operator = JoinOperator::Join(constraint.clone());
            }
        }
    }
}

impl VisitorMut for Hints {
    type Break = ();

    fn pre_visit_select(&mut self, select: &mut Select) -> ControlFlow<()> {
        if let Some(modifiers) = &select.select_modifiers {
            let calc = modifiers.sql_calc_found_rows;
            select.select_modifiers = calc.then(|| SelectModifiers {
                sql_calc_found_rows: true,
                ..Default::default()
            });
        }
        joins(&mut select.from);
        ControlFlow::Continue(())
    }

    fn pre_visit_table_factor(&mut self, factor: &mut TableFactor) -> ControlFlow<()> {
        if let TableFactor::Table {
            index_directives, ..
        } = factor
        {
            index_directives.clear();
        }
        ControlFlow::Continue(())
    }

    fn pre_visit_statement(&mut self, statement: &mut Ast) -> ControlFlow<()> {
        if let Ast::Insert(insert) = statement {
            insert.priority = None;
        }
        ControlFlow::Continue(())
    }
}

impl RewritePass for HintsPass {
    fn name(&self) -> &'static str {
        "hints"
    }

    fn rewrite(&self, stmt: &mut Statement, _cx: &PassContext) -> Result<Rewrite, DbError> {
        let _ = stmt.tree_mut().visit(&mut Hints);
        Ok(Rewrite::Continue)
    }
}
