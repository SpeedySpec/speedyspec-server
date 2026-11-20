//! `PgMultiDelete`: MySQL's `DELETE a, b FROM …` (and `DELETE FROM a USING …`) as one PostgreSQL statement: the
//! doomed rows are fixed in a CTE before any delete, each target table deletes its rows in a data-modifying CTE, and
//! the statement's single row counts them.

use speedyspec_data::{DbError, Statement};
use speedyspec_data_query::{PassContext, Rewrite, RewritePass};
use speedyspec_data_translate::object_name;
use sqlparser::ast::{
    Expr, FromTable, Ident, SelectItem, SetExpr, Statement as Ast, TableFactor, TableWithJoins,
};
use sqlparser::dialect::PostgreSqlDialect;
use sqlparser::parser::Parser;

use super::functions::unsupported;

pub(crate) struct PgMultiDelete;

fn labels(from: &[TableWithJoins]) -> Vec<(String, String)> {
    from.iter()
        .flat_map(|t| std::iter::once(&t.relation).chain(t.joins.iter().map(|j| &j.relation)))
        .filter_map(|f| match f {
            TableFactor::Table { name, alias, .. } => {
                let table = object_name(name);
                Some((
                    alias
                        .as_ref()
                        .map_or_else(|| table.clone(), |a| a.name.value.clone()),
                    table,
                ))
            }
            _ => None,
        })
        .collect()
}

fn quote(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

impl RewritePass for PgMultiDelete {
    fn name(&self) -> &'static str {
        "pg_multi_delete"
    }

    fn rewrite(&self, stmt: &mut Statement, _cx: &PassContext) -> Result<Rewrite, DbError> {
        let Ast::Delete(delete) = stmt.tree() else {
            return Ok(Rewrite::Continue);
        };
        let (targets, from): (Vec<String>, Vec<TableWithJoins>) =
            match (&delete.using, &delete.from) {
                (Some(using), FromTable::WithFromKeyword(f) | FromTable::WithoutKeyword(f)) => (
                    labels(f).into_iter().map(|(l, _)| l).collect(),
                    using.clone(),
                ),
                (None, FromTable::WithFromKeyword(f) | FromTable::WithoutKeyword(f))
                    if !delete.tables.is_empty() =>
                {
                    (delete.tables.iter().map(object_name).collect(), f.clone())
                }
                _ => return Ok(Rewrite::Continue),
            };
        let known = labels(&from);
        let resolved: Vec<(String, String)> = targets
            .iter()
            .map(|t| {
                known
                    .iter()
                    .find(|(l, _)| l.eq_ignore_ascii_case(t))
                    .cloned()
                    .ok_or_else(|| unsupported("DELETE of a table its FROM does not name"))
            })
            .collect::<Result<_, _>>()?;
        let mut tables: Vec<String> = Vec::new();
        for (_, table) in &resolved {
            if !tables.contains(table) {
                tables.push(table.clone());
            }
        }
        let mut sql = String::from("WITH \"__speedyspec_doomed\" AS (SELECT 1 FROM x)");
        for (d, table) in tables.iter().enumerate() {
            let members: Vec<String> = resolved
                .iter()
                .enumerate()
                .filter(|(_, (_, t))| t == table)
                .map(|(i, _)| format!("SELECT \"c{i}\" FROM \"__speedyspec_doomed\""))
                .collect();
            sql.push_str(&format!(
                ", \"__speedyspec_d{d}\" AS (DELETE FROM {} WHERE ctid IN ({}) RETURNING 1)",
                quote(table),
                members.join(" UNION ")
            ));
        }
        let counted: Vec<String> = (0..tables.len())
            .map(|d| format!("SELECT 1 FROM \"__speedyspec_d{d}\""))
            .collect();
        if counted.len() == 1 {
            sql.push_str(" SELECT count(*) AS \"__speedyspec_count\" FROM \"__speedyspec_d0\"");
        } else {
            sql.push_str(&format!(
                " SELECT count(*) AS \"__speedyspec_count\" FROM ({}) AS \"__speedyspec_all\"",
                counted.join(" UNION ALL ")
            ));
        }
        let mut tree = Parser::parse_sql(&PostgreSqlDialect {}, &sql)
            .map_err(|e| DbError::Parse {
                message: e.to_string(),
                line: 1,
                column: 1,
            })?
            .remove(0);
        if let Ast::Query(query) = &mut tree
            && let Some(with) = &mut query.with
            && let SetExpr::Select(select) = with.cte_tables[0].query.body.as_mut()
        {
            select.projection = resolved
                .iter()
                .enumerate()
                .map(|(i, (label, _))| SelectItem::ExprWithAlias {
                    expr: Expr::CompoundIdentifier(vec![Ident::new(label), Ident::new("ctid")]),
                    alias: Ident::new(format!("c{i}")),
                })
                .collect();
            select.from = from;
            select.selection = delete.selection.clone();
        }
        Ok(Rewrite::Split(vec![Statement::new(
            tree,
            stmt.params().to_vec(),
        )]))
    }
}
