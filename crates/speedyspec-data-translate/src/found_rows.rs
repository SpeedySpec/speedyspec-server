//! `SQL_CALC_FOUND_ROWS`: the count `FOUND_ROWS()` answers, as a second statement the engine runs beside the query.

use speedyspec_data::Statement;
use speedyspec_data_query::parse_one;
use sqlparser::ast::{SetExpr, Statement as Ast, TableFactor};

/// When the top-level `SELECT` carries `SQL_CALC_FOUND_ROWS`, removes the modifier and returns
/// `SELECT COUNT(*) FROM (<the query without its top-level ORDER BY, LIMIT and OFFSET>) AS speedyspec_found_rows`,
/// with the same params (numbered placeholders make the ones it no longer uses harmless).
pub fn take_found_rows(stmt: &mut Statement) -> Option<Statement> {
    let params = stmt.params().to_vec();
    let Ast::Query(query) = stmt.tree_mut() else {
        return None;
    };
    let SetExpr::Select(select) = &mut *query.body else {
        return None;
    };
    let modifiers = select.select_modifiers.as_mut()?;
    if !modifiers.sql_calc_found_rows {
        return None;
    }
    modifiers.sql_calc_found_rows = false;
    if *modifiers == Default::default() {
        select.select_modifiers = None;
    }
    let mut counted = (**query).clone();
    counted.order_by = None;
    counted.limit_clause = None;
    counted.fetch = None;
    // The COUNT(*) wrapper is text this function builds itself, then given the query.
    let (mut tree, _) = parse_one("SELECT COUNT(*) FROM (SELECT 1) AS speedyspec_found_rows")
        .ok()?
        .into_parts();
    if let Ast::Query(outer) = &mut tree
        && let SetExpr::Select(outer_select) = &mut *outer.body
        && let TableFactor::Derived { subquery, .. } = &mut outer_select.from[0].relation
    {
        **subquery = counted;
    }
    Some(Statement::new(tree, params))
}
