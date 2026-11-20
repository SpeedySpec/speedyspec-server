//! `sqlite_dml`: MySQL's row counts and insert ids from SQLite, and the multi-table and `REPLACE` statements SQLite
//! has no form for.

use speedyspec_data::{DbError, DialectKind, Introspector, Statement};
use speedyspec_data_query::{PassContext, Rewrite, RewritePass, parse_one};
use speedyspec_data_translate::{object_name, sql_ident};
use sqlparser::ast::{
    Assignment, AssignmentTarget, BinaryOperator, Delete, DoUpdate, Expr, FromTable, Ident,
    InTarget, Insert, JoinConstraint, JoinOperator, ObjectName, ObjectNamePart, OnConflict,
    OnConflictAction, OnInsert, Query, SelectItem, SetExpr, Statement as Ast, TableFactor,
    TableObject, TableWithJoins, Update, UpdateTableFromKind, Visit, Visitor,
};

pub struct SqliteDml;

/// Functions whose value differs per row or per call; an assignment from one always counts as a change.
const NONDETERMINISTIC: &[&str] = &[
    "rand",
    "uuid",
    "now",
    "sysdate",
    "curdate",
    "curtime",
    "utc_timestamp",
];

fn unsupported(construct: &str) -> DbError {
    DbError::Unsupported {
        construct: construct.into(),
        dialect: DialectKind::Sqlite,
    }
}

fn ident(name: &str) -> Ident {
    Ident::new(name)
}

fn and(left: Expr, right: Expr) -> Expr {
    Expr::BinaryOp {
        left: Box::new(left),
        op: BinaryOperator::And,
        right: Box::new(right),
    }
}

fn or(left: Expr, right: Expr) -> Expr {
    Expr::BinaryOp {
        left: Box::new(left),
        op: BinaryOperator::Or,
        right: Box::new(right),
    }
}

fn nested(expr: Expr) -> Expr {
    match expr {
        Expr::Nested(_) => expr,
        other => Expr::Nested(Box::new(other)),
    }
}

fn nondeterministic(expr: &Expr) -> bool {
    struct Find(bool);
    impl Visitor for Find {
        type Break = ();
        fn pre_visit_expr(&mut self, expr: &Expr) -> std::ops::ControlFlow<()> {
            if let Expr::Function(f) = expr
                && NONDETERMINISTIC.contains(&object_name(&f.name).to_ascii_lowercase().as_str())
            {
                self.0 = true;
            }
            std::ops::ControlFlow::Continue(())
        }
    }
    let mut find = Find(false);
    let _ = expr.visit(&mut find);
    find.0
}

/// `column IS NOT value COLLATE BINARY`: true when the assignment changes the row, a change in case included.
fn changed(column: Expr, value: &Expr) -> Expr {
    let simple = matches!(
        value,
        Expr::Value(_)
            | Expr::Identifier(_)
            | Expr::CompoundIdentifier(_)
            | Expr::Function(_)
            | Expr::Nested(_)
    );
    let value = if simple {
        value.clone()
    } else {
        nested(value.clone())
    };
    Expr::BinaryOp {
        left: Box::new(column),
        op: BinaryOperator::IsNot,
        right: Box::new(Expr::Collate {
            expr: Box::new(value),
            collation: ObjectName::from(vec![ident("BINARY")]),
        }),
    }
}

/// The OR of one `changed` term per assignment; `None` when an assigned value is nondeterministic. `qualifier`
/// names the target in a multi-table statement.
fn changed_rows(assignments: &[Assignment], qualifier: Option<&str>) -> Option<Expr> {
    if assignments.iter().any(|a| nondeterministic(&a.value)) {
        return None;
    }
    let mut terms = assignments.iter().filter_map(|a| {
        let AssignmentTarget::ColumnName(name) = &a.target else {
            return None;
        };
        let column = last_part(name)?;
        let column = match qualifier {
            Some(q) => Expr::CompoundIdentifier(vec![ident(q), column]),
            None => Expr::Identifier(column),
        };
        Some(changed(column, &a.value))
    });
    let first = terms.next()?;
    Some(terms.fold(first, or))
}

fn last_part(name: &ObjectName) -> Option<Ident> {
    match name.0.last()? {
        ObjectNamePart::Identifier(i) => Some(i.clone()),
        _ => None,
    }
}

fn qualifier(name: &ObjectName) -> Option<String> {
    (name.0.len() > 1).then(|| match &name.0[name.0.len() - 2] {
        ObjectNamePart::Identifier(i) => i.value.clone(),
        other => other.to_string(),
    })
}

/// The table a statement names, alias or name, and its table name.
fn label_of(factor: &TableFactor) -> Option<(String, String)> {
    match factor {
        TableFactor::Table { name, alias, .. } => {
            let table = object_name(name);
            Some((
                alias
                    .as_ref()
                    .map(|a| a.name.value.clone())
                    .unwrap_or_else(|| table.clone()),
                table,
            ))
        }
        _ => None,
    }
}

fn labels(from: &[TableWithJoins]) -> Vec<(String, String)> {
    from.iter()
        .flat_map(|t| std::iter::once(&t.relation).chain(t.joins.iter().map(|j| &j.relation)))
        .filter_map(label_of)
        .collect()
}

fn auto_increment(table: &str, introspector: Option<&dyn Introspector>) -> Result<bool, DbError> {
    Ok(match introspector {
        Some(i) => i
            .columns(table)?
            .iter()
            .any(|c| c.extra == "auto_increment"),
        None => false,
    })
}

fn returning_rowid() -> Option<Vec<SelectItem>> {
    Some(vec![SelectItem::UnnamedExpr(Expr::Identifier(ident(
        "rowid",
    )))])
}

fn values_rows(insert: &Insert) -> Option<&Vec<sqlparser::ast::Parens<Vec<Expr>>>> {
    match insert.source.as_deref()?.body.as_ref() {
        SetExpr::Values(values) => Some(&values.rows),
        _ => None,
    }
}

fn with_one_row(insert: &Insert, row: usize) -> Insert {
    let mut one = insert.clone();
    if let Some(source) = &mut one.source
        && let SetExpr::Values(values) = source.body.as_mut()
    {
        values.rows = vec![values.rows[row].clone()];
    }
    one
}

/// `SELECT <items> FROM <from> WHERE <selection>`, from a skeleton this pass parses itself.
fn select(
    items: Vec<SelectItem>,
    from: Vec<TableWithJoins>,
    selection: Option<Expr>,
) -> Result<Query, DbError> {
    let (tree, _) = parse_one("SELECT 1 FROM t")?.into_parts();
    let Ast::Query(mut query) = tree else {
        unreachable!("a SELECT parses as a query")
    };
    if let SetExpr::Select(s) = query.body.as_mut() {
        s.projection = items;
        s.from = from;
        s.selection = selection;
    }
    Ok(*query)
}

fn row_ids_in(
    table: &str,
    query: Query,
    params: &[speedyspec_data::DbValue],
) -> Result<Statement, DbError> {
    let (mut tree, _) =
        parse_one(&format!("DELETE FROM {} WHERE 1", sql_ident(table)))?.into_parts();
    if let Ast::Delete(delete) = &mut tree {
        delete.selection = Some(Expr::In {
            expr: Box::new(Expr::Identifier(ident("rowid"))),
            target: InTarget::Subquery(Box::new(query)),
            negated: false,
            global: false,
        });
    }
    Ok(Statement::new(tree, params.to_vec()))
}

impl SqliteDml {
    fn update(update: &mut Update) -> Result<(), DbError> {
        let target = label_of(&update.table.relation).map(|(label, _)| label);
        let multi = !update.table.joins.is_empty();
        for assignment in &mut update.assignments {
            if let AssignmentTarget::ColumnName(name) = &mut assignment.target
                && let Some(q) = qualifier(name)
            {
                if target
                    .as_deref()
                    .is_some_and(|t| !t.eq_ignore_ascii_case(&q))
                {
                    return Err(unsupported("UPDATE that sets columns of two tables"));
                }
                // SQLite's SET names the target's columns unqualified.
                if let Some(column) = last_part(name) {
                    *name = ObjectName::from(vec![column]);
                }
            }
        }
        let condition = update.selection.take();
        let had_where = condition.is_some();
        let mut parts: Vec<Expr> = Vec::new();
        if multi {
            let mut joins = std::mem::take(&mut update.table.joins).into_iter();
            let first = joins.next().expect("a join");
            let on = match first.join_operator {
                JoinOperator::Join(c) | JoinOperator::Inner(c) | JoinOperator::StraightJoin(c) => c,
                JoinOperator::CrossJoin(c) => c,
                JoinOperator::Left(_) | JoinOperator::LeftOuter(_) => {
                    return Err(unsupported("UPDATE … LEFT JOIN"));
                }
                _ => return Err(unsupported("UPDATE with this join")),
            };
            let on = match on {
                JoinConstraint::On(expr) => Some(expr),
                JoinConstraint::None => None,
                _ => return Err(unsupported("UPDATE … JOIN … USING")),
            };
            update.from = Some(UpdateTableFromKind::AfterSet(vec![TableWithJoins {
                relation: first.relation,
                joins: joins.collect(),
            }]));
            parts.extend(on.map(nested));
        }
        parts.extend(condition.map(nested));
        let qualifier = if multi { target.as_deref() } else { None };
        parts.extend(changed_rows(&update.assignments, qualifier).map(nested));
        // A single condition keeps its own form; several are each parenthesised and joined.
        let condition = match parts.len() {
            0 => None,
            1 if !multi => {
                let only = parts.remove(0);
                Some(match (&only, had_where) {
                    (Expr::Nested(inner), true) => (**inner).clone(),
                    _ => only,
                })
            }
            _ => parts.into_iter().reduce(and),
        };
        update.selection = condition;
        Ok(())
    }

    fn insert(stmt: &Statement, cx: &PassContext) -> Result<Rewrite, DbError> {
        let Ast::Insert(insert) = stmt.tree() else {
            unreachable!("an insert")
        };
        let TableObject::TableName(name) = &insert.table else {
            return Ok(Rewrite::Continue);
        };
        let table = object_name(name);
        if insert.replace_into {
            return Self::replace(insert, &table, stmt, cx);
        }
        let rows = values_rows(insert).map_or(1, Vec::len);
        let auto = auto_increment(&table, cx.introspector)?;
        let mut insert = insert.clone();
        let upsert = matches!(
            &insert.on,
            Some(OnInsert::OnConflict(OnConflict {
                action: OnConflictAction::DoUpdate(_),
                ..
            }))
        );
        if let Some(OnInsert::OnConflict(OnConflict {
            action:
                OnConflictAction::DoUpdate(DoUpdate {
                    assignments,
                    selection,
                }),
            ..
        })) = &mut insert.on
            && selection.is_none()
        {
            *selection = changed_rows(assignments, None);
        }
        if auto && (upsert || rows > 1) {
            insert.returning = returning_rowid();
        }
        if upsert && rows > 1 {
            let parts = (0..rows)
                .map(|row| {
                    Statement::new(
                        Ast::Insert(with_one_row(&insert, row)),
                        stmt.params().to_vec(),
                    )
                })
                .collect();
            return Ok(Rewrite::Split(parts));
        }
        Ok(Rewrite::Split(vec![Statement::new(
            Ast::Insert(insert),
            stmt.params().to_vec(),
        )]))
    }

    /// MySQL's `REPLACE`: the rows that clash on a unique key are deleted, then the row is inserted, and both count.
    fn replace(
        insert: &Insert,
        table: &str,
        stmt: &Statement,
        cx: &PassContext,
    ) -> Result<Rewrite, DbError> {
        let Some(rows) = values_rows(insert) else {
            return Err(unsupported("REPLACE ... SELECT"));
        };
        let mut columns: Vec<String> = insert.columns.iter().map(object_name).collect();
        let indexes = match cx.introspector {
            Some(i) => {
                if columns.is_empty() {
                    columns = i.columns(table)?.into_iter().map(|c| c.name).collect();
                }
                i.indexes(table)?
            }
            None => Vec::new(),
        };
        let keys: Vec<Vec<usize>> = indexes
            .iter()
            .filter(|i| i.unique)
            .filter_map(|i| {
                i.columns
                    .iter()
                    .map(|(c, _)| columns.iter().position(|k| k.eq_ignore_ascii_case(c)))
                    .collect::<Option<Vec<usize>>>()
            })
            .collect();
        let mut plain = insert.clone();
        plain.replace_into = false;
        let mut out = Vec::new();
        for (r, row) in rows.iter().enumerate() {
            if row.content.len() != columns.len() {
                return Err(DbError::Engine {
                    engine: String::new(),
                    code: Some("1136".into()),
                    message: format!("Column count doesn't match value count at row {}", r + 1),
                });
            }
            let clashes = keys.iter().map(|key| {
                let mut terms = key.iter().map(|&p| Expr::BinaryOp {
                    left: Box::new(Expr::Identifier(ident(&columns[p]))),
                    op: BinaryOperator::Eq,
                    right: Box::new(row.content[p].clone()),
                });
                let first = terms.next().expect("an index has columns");
                nested(terms.fold(first, and))
            });
            if let Some(selection) = clashes.reduce(or) {
                let (mut tree, _) =
                    parse_one(&format!("DELETE FROM {} WHERE 1", sql_ident(table)))?.into_parts();
                if let Ast::Delete(delete) = &mut tree {
                    delete.selection = Some(selection);
                }
                out.push(Statement::new(tree, stmt.params().to_vec()));
            }
            out.push(Statement::new(
                Ast::Insert(with_one_row(&plain, r)),
                stmt.params().to_vec(),
            ));
        }
        Ok(Rewrite::Split(out))
    }

    /// `DELETE a, b FROM …` and `DELETE FROM a, b USING …`: the rows of each target fixed first, then deleted.
    fn delete(delete: &Delete, stmt: &Statement) -> Result<Rewrite, DbError> {
        let (targets, from): (Vec<String>, Vec<TableWithJoins>) =
            match (&delete.using, &delete.from) {
                (
                    Some(using),
                    FromTable::WithFromKeyword(from) | FromTable::WithoutKeyword(from),
                ) => (
                    labels(from).into_iter().map(|(l, _)| l).collect(),
                    using.clone(),
                ),
                (None, FromTable::WithFromKeyword(from) | FromTable::WithoutKeyword(from))
                    if !delete.tables.is_empty() =>
                {
                    (
                        delete.tables.iter().map(object_name).collect(),
                        from.clone(),
                    )
                }
                _ => return Ok(Rewrite::Continue),
            };
        let known = labels(&from);
        let resolved = targets
            .iter()
            .map(|t| {
                known
                    .iter()
                    .find(|(label, _)| label.eq_ignore_ascii_case(t))
                    .cloned()
                    .ok_or_else(|| unsupported("DELETE of a table its FROM does not name"))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let row_id = |label: &str| Expr::CompoundIdentifier(vec![ident(label), ident("rowid")]);
        let params = stmt.params();
        if let [(label, table)] = resolved.as_slice() {
            let query = select(
                vec![SelectItem::UnnamedExpr(row_id(label))],
                from,
                delete.selection.clone(),
            )?;
            return Ok(Rewrite::Split(vec![row_ids_in(table, query, params)?]));
        }
        let items = resolved
            .iter()
            .enumerate()
            .map(|(i, (label, _))| SelectItem::ExprWithAlias {
                expr: row_id(label),
                alias: ident(&format!("r{i}")),
            })
            .collect();
        let query = select(items, from, delete.selection.clone())?;
        let (mut create, _) =
            parse_one("CREATE TEMPORARY TABLE speedyspec_delete AS SELECT 1")?.into_parts();
        if let Ast::CreateTable(c) = &mut create {
            c.query = Some(Box::new(query));
        }
        let mut out = vec![
            parse_one("DROP TABLE IF EXISTS temp.speedyspec_delete")?,
            Statement::new(create, params.to_vec()),
        ];
        for (i, (_, table)) in resolved.iter().enumerate() {
            out.push(parse_one(&format!(
                "DELETE FROM {} WHERE rowid IN (SELECT r{i} FROM temp.speedyspec_delete)",
                sql_ident(table)
            ))?);
        }
        out.push(parse_one("DROP TABLE temp.speedyspec_delete")?);
        Ok(Rewrite::Split(out))
    }
}

impl RewritePass for SqliteDml {
    fn name(&self) -> &'static str {
        "sqlite_dml"
    }

    fn rewrite(&self, stmt: &mut Statement, cx: &PassContext) -> Result<Rewrite, DbError> {
        match stmt.tree_mut() {
            Ast::Update(update) => {
                Self::update(update)?;
                Ok(Rewrite::Continue)
            }
            Ast::Insert(_) => Self::insert(stmt, cx),
            Ast::Delete(delete) => {
                let delete = delete.clone();
                Self::delete(&delete, stmt)
            }
            _ => Ok(Rewrite::Continue),
        }
    }
}
