//! What PostgreSQL runs for one MySQL statement, and how the store reads each statement's count.

use speedyspec_data::{EmittedSql, EmulatedOp};
use sqlparser::ast::{Expr, Query, SelectItem, SetExpr, Statement as Ast};

/// The column of an `INSERT ... RETURNING` that carries the generated id.
pub const ID_ALIAS: &str = "__speedyspec_id";
/// The column of an upsert's `RETURNING` that says whether the row was inserted.
pub const INSERTED_ALIAS: &str = "__speedyspec_inserted";
/// The column of a statement whose single row is the affected-row count.
pub const COUNT_ALIAS: &str = "__speedyspec_count";
/// The column of a hidden statement that moves a sequence.
pub const SEQUENCE_ALIAS: &str = "__speedyspec_sequence";

/// How the store reads an emitted statement's result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowCount {
    /// A result set for the caller.
    Rows,
    /// The command's affected-row count.
    Affected,
    /// `RETURNING` the generated ids (`ID_ALIAS`).
    Returning,
    /// `RETURNING` whether each row was inserted (`INSERTED_ALIAS`): 1 per insert, 2 per changed update.
    Upsert,
    /// One row whose `COUNT_ALIAS` is the count.
    CountColumn,
    /// Run for its effect only.
    Hidden,
    /// The count `FOUND_ROWS()` reports after the query before it.
    FoundRows,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PgEmitted {
    pub sql: EmittedSql,
    pub count: RowCount,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PgPlan {
    Sql(Vec<PgEmitted>),
    Emulated(EmulatedOp),
}

fn names(items: &[SelectItem]) -> Vec<String> {
    items
        .iter()
        .filter_map(|i| match i {
            SelectItem::ExprWithAlias { alias, .. } => Some(alias.value.clone()),
            SelectItem::UnnamedExpr(Expr::Identifier(i)) => Some(i.value.clone()),
            _ => None,
        })
        .collect()
}

fn projection(query: &Query) -> Vec<String> {
    match query.body.as_ref() {
        SetExpr::Select(select) => names(&select.projection),
        _ => Vec::new(),
    }
}

/// How `tree`, as emitted, is counted.
pub(crate) fn count_of(tree: &Ast) -> RowCount {
    let has = |list: &[String], alias: &str| list.iter().any(|n| n == alias);
    match tree {
        Ast::Query(query) => {
            let cols = projection(query);
            if has(&cols, SEQUENCE_ALIAS) {
                RowCount::Hidden
            } else if has(&cols, COUNT_ALIAS) {
                RowCount::CountColumn
            } else {
                RowCount::Rows
            }
        }
        Ast::Insert(sqlparser::ast::Insert {
            returning: Some(r), ..
        })
        | Ast::Update(sqlparser::ast::Update {
            returning: Some(r), ..
        })
        | Ast::Delete(sqlparser::ast::Delete {
            returning: Some(r), ..
        }) => {
            let cols = names(r);
            if has(&cols, INSERTED_ALIAS) {
                RowCount::Upsert
            } else if has(&cols, ID_ALIAS) {
                RowCount::Returning
            } else {
                RowCount::Rows
            }
        }
        Ast::ShowVariable { .. }
        | Ast::ShowVariables { .. }
        | Ast::ShowTables { .. }
        | Ast::ShowColumns { .. } => RowCount::Rows,
        _ => RowCount::Affected,
    }
}

pub(crate) fn emitted(sql: EmittedSql, tree: &Ast) -> PgEmitted {
    PgEmitted {
        count: count_of(tree),
        sql,
    }
}
