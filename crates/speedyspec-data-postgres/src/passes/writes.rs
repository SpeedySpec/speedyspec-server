//! MySQL's write counts and ids on PostgreSQL: `PgReplace`, `PgUpsert`, `PgInsertId` and `PgChangedRows`.

use std::rc::Rc;

use speedyspec_data::{DbError, Statement};
use speedyspec_data_query::{PassContext, Rewrite, RewritePass, parse_one};
use speedyspec_data_translate::{TypeFamily, object_name, sql_ident};
use sqlparser::ast::{
    Assignment, AssignmentTarget, BinaryOperator, DoUpdate, Expr, Ident, Insert, ObjectName,
    OnConflict, OnConflictAction, OnInsert, SelectItem, SetExpr, Statement as Ast, TableObject,
};

use super::functions::{template, unsupported};
use super::params::{copy_params, renumber};
use crate::plan::{ID_ALIAS, INSERTED_ALIAS};
use crate::schema::{SchemaCache, tables_in};

fn insert_table(insert: &Insert) -> Option<String> {
    match &insert.table {
        TableObject::TableName(name) => Some(object_name(name)),
        _ => None,
    }
}

fn column_names(insert: &Insert) -> Vec<String> {
    insert.columns.iter().map(object_name).collect()
}

fn collate_c(expr: Expr) -> Expr {
    Expr::Collate {
        expr: Box::new(expr),
        collation: ObjectName::from(vec![Ident::new("C")]),
    }
}

/// The unique indexes (primary key included) whose every column the insert names.
fn covered_keys(
    schema: &SchemaCache,
    table: &str,
    columns: &[String],
) -> Vec<(String, Vec<String>)> {
    schema
        .indexes(table)
        .map(|indexes| {
            indexes
                .iter()
                .filter(|i| i.unique)
                .filter(|i| {
                    i.columns
                        .iter()
                        .all(|(c, _)| columns.iter().any(|k| k.eq_ignore_ascii_case(c)))
                })
                .map(|i| {
                    (
                        i.name.clone(),
                        i.columns.iter().map(|(c, _)| c.clone()).collect(),
                    )
                })
                .collect()
        })
        .unwrap_or_default()
}

fn is_text(schema: &SchemaCache, table: &str, column: &str) -> bool {
    let tables = [(table.to_string(), table.to_string())];
    schema
        .type_of(&tables, &Expr::Identifier(Ident::new(column)))
        .is_some_and(|t| matches!(t.family(), TypeFamily::Text | TypeFamily::Enum))
}

/// `(cols) IS DISTINCT FROM (exprs)`, one column as a scalar comparison; text compared byte for byte, so a change
/// of case counts as MySQL's row comparison counts it.
fn changed(columns: Vec<(Expr, bool)>, values: Vec<Expr>) -> Expr {
    let wrap = |(e, text): (Expr, bool)| if text { collate_c(e) } else { e };
    let pairs: Vec<(Expr, Expr)> = columns
        .into_iter()
        .zip(values)
        .map(|((c, text), v)| (wrap((c, text)), wrap((v, text))))
        .collect();
    let (left, right): (Vec<Expr>, Vec<Expr>) = pairs.into_iter().unzip();
    if left.len() == 1 {
        Expr::IsDistinctFrom(
            Box::new(left.into_iter().next().unwrap()),
            Box::new(right.into_iter().next().unwrap()),
        )
    } else {
        Expr::IsDistinctFrom(Box::new(Expr::Tuple(left)), Box::new(Expr::Tuple(right)))
    }
}

fn last_ident(name: &ObjectName) -> Option<Ident> {
    name.0.last().and_then(|p| p.as_ident().cloned())
}

pub(crate) struct PgReplace {
    pub(crate) schema: Rc<SchemaCache>,
}

impl RewritePass for PgReplace {
    fn name(&self) -> &'static str {
        "pg_replace"
    }

    fn rewrite(&self, stmt: &mut Statement, _cx: &PassContext) -> Result<Rewrite, DbError> {
        let Ast::Insert(insert) = stmt.tree() else {
            return Ok(Rewrite::Continue);
        };
        if !insert.replace_into {
            return Ok(Rewrite::Continue);
        }
        let Some(table) = insert_table(insert) else {
            return Ok(Rewrite::Continue);
        };
        let rows = match insert.source.as_deref().map(|q| q.body.as_ref()) {
            Some(SetExpr::Values(values)) => values.rows.clone(),
            _ => return Err(unsupported("REPLACE ... SELECT")),
        };
        let mut columns = column_names(insert);
        if columns.is_empty() {
            columns = self
                .schema
                .columns(&table)
                .map(|c| c.iter().map(|c| c.name.clone()).collect())
                .unwrap_or_default();
        }
        let keys = covered_keys(&self.schema, &table, &columns);
        let mut out = Vec::new();
        let mut clashes: Vec<Expr> = Vec::new();
        for row in &rows {
            if row.content.len() != columns.len() {
                return Err(DbError::Engine {
                    engine: String::new(),
                    code: Some("1136".into()),
                    message: "Column count doesn't match value count at row 1".into(),
                });
            }
            for (_, key) in &keys {
                let terms: Vec<Expr> = key
                    .iter()
                    .map(|k| {
                        let p = columns
                            .iter()
                            .position(|c| c.eq_ignore_ascii_case(k))
                            .expect("a covered key");
                        Expr::BinaryOp {
                            left: Box::new(Expr::Identifier(Ident::new(k))),
                            op: BinaryOperator::Eq,
                            right: Box::new(row.content[p].clone()),
                        }
                    })
                    .collect();
                let all = terms.into_iter().reduce(|a, b| Expr::BinaryOp {
                    left: Box::new(a),
                    op: BinaryOperator::And,
                    right: Box::new(b),
                });
                clashes.extend(all);
            }
        }
        if let Some(mut selection) = clashes.into_iter().reduce(|a, b| Expr::BinaryOp {
            left: Box::new(Expr::Nested(Box::new(a))),
            op: BinaryOperator::Or,
            right: Box::new(Expr::Nested(Box::new(b))),
        }) {
            let params = renumber(&mut selection, stmt.params());
            let (mut tree, _) =
                parse_one(&format!("DELETE FROM {} WHERE 1", sql_ident(&table)))?.into_parts();
            if let Ast::Delete(delete) = &mut tree {
                delete.selection = Some(selection);
            }
            out.push(Statement::new(tree, params));
        }
        let mut plain = insert.clone();
        plain.replace_into = false;
        out.push(Statement::new(Ast::Insert(plain), stmt.params().to_vec()));
        Ok(Rewrite::Split(out))
    }
}

pub(crate) struct PgUpsert {
    pub(crate) schema: Rc<SchemaCache>,
}

impl RewritePass for PgUpsert {
    fn name(&self) -> &'static str {
        "pg_upsert"
    }

    fn rewrite(&self, stmt: &mut Statement, _cx: &PassContext) -> Result<Rewrite, DbError> {
        let mut params = stmt.params().to_vec();
        let Ast::Insert(insert) = stmt.tree_mut() else {
            return Ok(Rewrite::Continue);
        };
        let Some(table) = insert_table(insert) else {
            return Ok(Rewrite::Continue);
        };
        let Some(OnInsert::OnConflict(OnConflict {
            action: OnConflictAction::DoUpdate(_),
            ..
        })) = &insert.on
        else {
            return Ok(Rewrite::Continue);
        };
        let keys = covered_keys(&self.schema, &table, &column_names(insert));
        let target = match keys.as_slice() {
            // No unique key among the inserted columns: MySQL never finds a duplicate, it just inserts.
            [] => {
                insert.on = None;
                return Ok(Rewrite::Continue);
            }
            [(_, columns)] => columns.clone(),
            several => {
                let names: Vec<&str> = several.iter().map(|(n, _)| n.as_str()).collect();
                return Err(unsupported(&format!(
                    "ON DUPLICATE KEY UPDATE with several unique keys among the inserted columns: {}",
                    names.join(", ")
                )));
            }
        };
        let Some(OnInsert::OnConflict(conflict)) = &mut insert.on else {
            unreachable!("checked above")
        };
        conflict.conflict_target = Some(sqlparser::ast::ConflictTarget::Columns {
            columns: target.iter().map(Ident::new).collect(),
            predicate: None,
        });
        let OnConflictAction::DoUpdate(DoUpdate {
            assignments,
            selection,
        }) = &mut conflict.action
        else {
            unreachable!("checked above")
        };
        let mut columns = Vec::new();
        let mut values = Vec::new();
        for Assignment { target: t, value } in assignments.iter() {
            let AssignmentTarget::ColumnName(name) = t else {
                continue;
            };
            let Some(column) = last_ident(name) else {
                continue;
            };
            let text = is_text(&self.schema, &table, &column.value);
            columns.push((
                Expr::CompoundIdentifier(vec![Ident::new(&table), column]),
                text,
            ));
            let mut value = value.clone();
            copy_params(&mut value, &mut params);
            values.push(value);
        }
        if !columns.is_empty() && selection.is_none() {
            *selection = Some(changed(columns, values));
        }
        let inserted = template("(xmax = 0)", vec![]);
        let item = SelectItem::ExprWithAlias {
            expr: inserted,
            alias: Ident::new(INSERTED_ALIAS),
        };
        insert.returning.get_or_insert_with(Vec::new).push(item);
        *stmt.params_mut() = params;
        Ok(Rewrite::Continue)
    }
}

pub(crate) struct PgInsertId {
    pub(crate) schema: Rc<SchemaCache>,
}

impl RewritePass for PgInsertId {
    fn name(&self) -> &'static str {
        "pg_insert_id"
    }

    fn rewrite(&self, stmt: &mut Statement, _cx: &PassContext) -> Result<Rewrite, DbError> {
        let Ast::Insert(insert) = stmt.tree_mut() else {
            return Ok(Rewrite::Continue);
        };
        let Some(table) = insert_table(insert) else {
            return Ok(Rewrite::Continue);
        };
        let Some(id) = self.schema.columns(&table).and_then(|c| {
            c.iter()
                .find(|c| c.extra == "auto_increment")
                .map(|c| c.name.clone())
        }) else {
            return Ok(Rewrite::Continue);
        };
        let item = SelectItem::ExprWithAlias {
            expr: Expr::Identifier(Ident::new(&id)),
            alias: Ident::new(ID_ALIAS),
        };
        insert
            .returning
            .get_or_insert_with(Vec::new)
            .insert(0, item);
        let explicit = column_names(insert)
            .iter()
            .any(|c| c.eq_ignore_ascii_case(&id));
        if !explicit {
            return Ok(Rewrite::Continue);
        }
        // MySQL's counter moves past an id given explicitly; PostgreSQL's sequence must be told.
        let t = format!("\"{}\"", table.replace('"', "\"\""));
        let seq = format!(
            "pg_get_serial_sequence('{}', '{}')",
            t.replace('\'', "''"),
            id.replace('\'', "''")
        );
        let c = format!("\"{}\"", id.replace('"', "\"\""));
        let sql = format!(
            "SELECT setval({seq}, GREATEST((SELECT COALESCE(MAX({c}), 0) FROM {t}), COALESCE(pg_sequence_last_value({seq}), 0), 1)) AS \"__speedyspec_sequence\""
        );
        let setval =
            sqlparser::parser::Parser::parse_sql(&sqlparser::dialect::PostgreSqlDialect {}, &sql)
                .map_err(|e| DbError::Parse {
                    message: e.to_string(),
                    line: 1,
                    column: 1,
                })?
                .remove(0);
        let first = Statement::new(stmt.tree().clone(), stmt.params().to_vec());
        Ok(Rewrite::Split(vec![
            first,
            Statement::new(setval, Vec::new()),
        ]))
    }
}

pub(crate) struct PgChangedRows {
    pub(crate) schema: Rc<SchemaCache>,
}

impl RewritePass for PgChangedRows {
    fn name(&self) -> &'static str {
        "pg_changed_rows"
    }

    fn rewrite(&self, stmt: &mut Statement, _cx: &PassContext) -> Result<Rewrite, DbError> {
        let tables = tables_in(stmt.tree());
        let mut params = stmt.params().to_vec();
        let Ast::Update(update) = stmt.tree_mut() else {
            return Ok(Rewrite::Continue);
        };
        let Some((label, table)) = tables.first().cloned() else {
            return Ok(Rewrite::Continue);
        };
        let multi = !update.table.joins.is_empty() || update.from.is_some();
        let mut columns = Vec::new();
        let mut values = Vec::new();
        for a in &update.assignments {
            let AssignmentTarget::ColumnName(name) = &a.target else {
                continue;
            };
            let Some(column) = last_ident(name) else {
                continue;
            };
            let text = is_text(&self.schema, &table, &column.value);
            let reference = if multi {
                Expr::CompoundIdentifier(vec![Ident::new(&label), column])
            } else {
                Expr::Identifier(column)
            };
            columns.push((reference, text));
            let mut value = a.value.clone();
            copy_params(&mut value, &mut params);
            values.push(value);
        }
        if columns.is_empty() {
            return Ok(Rewrite::Continue);
        }
        let guard = changed(columns, values);
        update.selection = Some(match update.selection.take() {
            Some(w) => Expr::BinaryOp {
                left: Box::new(Expr::Nested(Box::new(w))),
                op: BinaryOperator::And,
                right: Box::new(guard),
            },
            None => guard,
        });
        *stmt.params_mut() = params;
        Ok(Rewrite::Continue)
    }
}
