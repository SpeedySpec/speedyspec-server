//! `InsertPass`: MySQL's insert forms in the portable `ON CONFLICT` shape PostgreSQL and SQLite share.

use std::ops::ControlFlow;

use speedyspec_data::{DbError, Statement};
use speedyspec_data_query::{PassContext, Rewrite, RewritePass, parse_one};
use sqlparser::ast::{
    AssignmentTarget, DoUpdate, Expr, FunctionArg, FunctionArgExpr, FunctionArguments, Ident,
    OnConflict, OnConflictAction, OnInsert, SetExpr, Statement as Ast, VisitMut, VisitorMut,
};

/// `INSERT … SET a = 1` to `(a) VALUES (1)`; `ON DUPLICATE KEY UPDATE` to a target-less `ON CONFLICT DO UPDATE SET`,
/// `VALUES(col)` and the MySQL 8 row alias (`AS new … new.col`) to `excluded.col`; `INSERT IGNORE` to
/// `ON CONFLICT DO NOTHING`.
pub struct InsertPass;

/// `VALUES(col)` or `<alias>.col` to `excluded.col`.
struct Excluded<'a> {
    alias: Option<&'a str>,
}

fn excluded(column: Ident) -> Expr {
    Expr::CompoundIdentifier(vec![Ident::new("excluded"), column])
}

impl VisitorMut for Excluded<'_> {
    type Break = ();

    fn post_visit_expr(&mut self, expr: &mut Expr) -> ControlFlow<()> {
        match expr {
            Expr::Function(f) if f.name.to_string().eq_ignore_ascii_case("VALUES") => {
                if let FunctionArguments::List(list) = &f.args
                    && let [FunctionArg::Unnamed(FunctionArgExpr::Expr(Expr::Identifier(col)))] =
                        list.args.as_slice()
                {
                    *expr = excluded(col.clone());
                }
            }
            Expr::CompoundIdentifier(parts)
                if parts.len() == 2
                    && self
                        .alias
                        .is_some_and(|a| parts[0].value.eq_ignore_ascii_case(a)) =>
            {
                *expr = excluded(parts[1].clone());
            }
            _ => {}
        }
        ControlFlow::Continue(())
    }
}

impl RewritePass for InsertPass {
    fn name(&self) -> &'static str {
        "insert"
    }

    fn rewrite(&self, stmt: &mut Statement, _cx: &PassContext) -> Result<Rewrite, DbError> {
        let Ast::Insert(insert) = stmt.tree_mut() else {
            return Ok(Rewrite::Continue);
        };
        if insert.source.is_none() && !insert.assignments.is_empty() {
            // The VALUES skeleton is text this pass builds itself, then filled with the assignments' expressions.
            let Ast::Insert(skeleton) = parse_one("INSERT INTO t (a) VALUES (1)")?.into_parts().0
            else {
                unreachable!("an INSERT parses as one")
            };
            let mut source = skeleton.source.expect("a VALUES source");
            let assignments = std::mem::take(&mut insert.assignments);
            if let SetExpr::Values(values) = &mut *source.body {
                values.rows[0].content = assignments.iter().map(|a| a.value.clone()).collect();
            }
            insert.columns = assignments
                .iter()
                .filter_map(|a| match &a.target {
                    AssignmentTarget::ColumnName(name) => Some(name.clone()),
                    AssignmentTarget::Tuple(_) => None,
                })
                .collect();
            insert.source = Some(source);
        }
        let alias = insert
            .insert_alias
            .take()
            .map(|a| crate::names::object_name(&a.row_alias));
        if let Some(OnInsert::DuplicateKeyUpdate(assignments)) = insert.on.take() {
            let mut update = DoUpdate {
                assignments,
                selection: None,
            };
            let mut v = Excluded {
                alias: alias.as_deref(),
            };
            for a in &mut update.assignments {
                let _ = a.value.visit(&mut v);
            }
            insert.on = Some(OnInsert::OnConflict(OnConflict {
                conflict_target: None,
                action: OnConflictAction::DoUpdate(update),
            }));
        } else if insert.ignore {
            insert.ignore = false;
            if insert.on.is_none() {
                insert.on = Some(OnInsert::OnConflict(OnConflict {
                    conflict_target: None,
                    action: OnConflictAction::DoNothing,
                }));
            }
        }
        Ok(Rewrite::Continue)
    }
}
