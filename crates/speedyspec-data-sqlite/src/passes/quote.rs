//! `sqlite_quote`: every table, column, index and alias name double-quoted, so a MySQL name that is a SQLite keyword
//! (or differs only in case from one) still names what it named.

use std::ops::ControlFlow;

use speedyspec_data::{DbError, Statement};
use speedyspec_data_query::{PassContext, Rewrite, RewritePass};
use sqlparser::ast::{
    AlterTableOperation, ColumnDef, ColumnOption, Expr, Ident, Statement as Ast, TableFactor,
    VisitMut, VisitorMut,
};

/// Names SQLite gives meaning to, which quoting would turn into ordinary identifiers.
const UNQUOTED: &[&str] = &[
    "rowid",
    "excluded",
    "temp",
    "main",
    "sqlite_sequence",
    "new",
    "old",
];

pub struct SqliteQuote;

fn bare(ident: &mut Ident) {
    ident.quote_style = None;
}

fn unquote_collations(columns: &mut [ColumnDef]) {
    for column in columns {
        for option in &mut column.options {
            if let ColumnOption::Collation(name) = &mut option.option {
                name.0.iter_mut().filter_map(ident_mut).for_each(bare);
            }
        }
    }
}

fn ident_mut(part: &mut sqlparser::ast::ObjectNamePart) -> Option<&mut Ident> {
    match part {
        sqlparser::ast::ObjectNamePart::Identifier(ident) => Some(ident),
        _ => None,
    }
}

struct Quote;

impl VisitorMut for Quote {
    type Break = ();

    fn pre_visit_ident(&mut self, ident: &mut Ident) -> ControlFlow<()> {
        let keep = UNQUOTED
            .iter()
            .any(|u| u.eq_ignore_ascii_case(&ident.value));
        if !keep {
            ident.quote_style = Some('"');
        }
        ControlFlow::Continue(())
    }

    fn pre_visit_table_factor(&mut self, factor: &mut TableFactor) -> ControlFlow<()> {
        if let TableFactor::Table {
            alias: Some(alias), ..
        } = factor
        {
            alias.explicit = true;
        }
        ControlFlow::Continue(())
    }

    fn post_visit_expr(&mut self, expr: &mut Expr) -> ControlFlow<()> {
        match expr {
            Expr::Function(f) => f.name.0.iter_mut().filter_map(ident_mut).for_each(bare),
            Expr::Collate { collation, .. } => {
                collation.0.iter_mut().filter_map(ident_mut).for_each(bare)
            }
            _ => {}
        }
        ControlFlow::Continue(())
    }
}

impl RewritePass for SqliteQuote {
    fn name(&self) -> &'static str {
        "sqlite_quote"
    }

    fn rewrite(&self, stmt: &mut Statement, _cx: &PassContext) -> Result<Rewrite, DbError> {
        let tree = stmt.tree_mut();
        let _ = tree.visit(&mut Quote);
        match tree {
            Ast::CreateTable(create) => unquote_collations(&mut create.columns),
            Ast::AlterTable(alter) => {
                for op in &mut alter.operations {
                    if let AlterTableOperation::AddColumn { column_def, .. } = op {
                        unquote_collations(std::slice::from_mut(column_def));
                    }
                }
            }
            _ => {}
        }
        Ok(Rewrite::Continue)
    }
}
