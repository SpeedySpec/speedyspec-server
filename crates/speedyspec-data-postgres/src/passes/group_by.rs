//! `PgLooseGroupBy`: MySQL lets a grouped query read a column that is neither grouped nor functionally dependent on
//! a grouped primary key, giving any row's value; PostgreSQL refuses it, so such a column reads `any_value(..)`.

use std::ops::ControlFlow;
use std::rc::Rc;

use speedyspec_data::{DbError, Statement};
use speedyspec_data_query::{PassContext, Rewrite, RewritePass};
use speedyspec_data_translate::object_name;
use sqlparser::ast::{
    Expr, GroupByExpr, Ident, Query, Select, SelectItem, SelectItemQualifiedWildcardKind, SetExpr,
    TableFactor, Value, VisitMut, VisitorMut,
};

use super::functions::template;
use crate::schema::SchemaCache;

pub(crate) struct PgLooseGroupBy {
    pub(crate) schema: Rc<SchemaCache>,
}

const AGGREGATES: &[&str] = &[
    "count",
    "sum",
    "avg",
    "min",
    "max",
    "group_concat",
    "string_agg",
    "any_value",
    "bit_and",
    "bit_or",
    "bit_xor",
    "std",
    "stddev",
    "stddev_pop",
    "stddev_samp",
    "variance",
    "var_pop",
    "var_samp",
    "json_arrayagg",
    "json_objectagg",
    "array_agg",
];

fn is_aggregate(expr: &Expr) -> bool {
    matches!(expr, Expr::Function(f) if f.over.is_none() && AGGREGATES.contains(&object_name(&f.name).to_ascii_lowercase().as_str()))
}

fn take(expr: &mut Expr) -> Expr {
    std::mem::replace(expr, Expr::value(Value::Null))
}

/// One SELECT's view: its tables, what is grouped, and which tables' primary keys are wholly grouped.
struct Grouping<'a> {
    schema: &'a SchemaCache,
    tables: Vec<(String, String)>,
    grouped: Vec<Expr>,
}

impl Grouping<'_> {
    /// (table, column) a column reference names.
    fn column(&self, expr: &Expr) -> Option<(String, String)> {
        match expr {
            Expr::Identifier(c) => self
                .tables
                .iter()
                .find(|(_, t)| self.schema.resolve(t, &c.value).is_some())
                .map(|(_, t)| (t.clone(), c.value.to_ascii_lowercase()))
                .or_else(|| {
                    self.tables
                        .first()
                        .map(|(_, t)| (t.clone(), c.value.to_ascii_lowercase()))
                }),
            Expr::CompoundIdentifier(parts) if parts.len() >= 2 => self
                .tables
                .iter()
                .find(|(l, _)| l.eq_ignore_ascii_case(&parts[parts.len() - 2].value))
                .map(|(_, t)| (t.clone(), parts[parts.len() - 1].value.to_ascii_lowercase())),
            _ => None,
        }
    }

    fn is_grouped(&self, expr: &Expr) -> bool {
        let text = expr.to_string().to_ascii_lowercase();
        if self
            .grouped
            .iter()
            .any(|g| g.to_string().to_ascii_lowercase() == text)
        {
            return true;
        }
        let Some(col) = self.column(expr) else {
            return false;
        };
        if self
            .grouped
            .iter()
            .any(|g| self.column(g).as_ref() == Some(&col))
        {
            return true;
        }
        self.key_grouped(&col.0)
    }

    /// Whether every column of the table's primary key is grouped: then each of its columns has one value per group.
    fn key_grouped(&self, table: &str) -> bool {
        let Some(indexes) = self.schema.indexes(table) else {
            return false;
        };
        let Some(primary) = indexes.iter().find(|i| i.name == "PRIMARY") else {
            return false;
        };
        primary.columns.iter().all(|(c, _)| {
            self.grouped.iter().any(|g| {
                self.column(g)
                    .is_some_and(|(t, gc)| t == table && gc.eq_ignore_ascii_case(c))
            })
        })
    }
}

/// Wraps the ungrouped columns of an expression outside aggregates, grouped expressions and subqueries.
struct Wrap<'a, 'b> {
    grouping: &'a Grouping<'b>,
    shields: Vec<bool>,
}

impl VisitorMut for Wrap<'_, '_> {
    type Break = ();

    fn pre_visit_expr(&mut self, expr: &mut Expr) -> ControlFlow<()> {
        let shield = is_aggregate(expr)
            || matches!(expr, Expr::Subquery(_) | Expr::Exists { .. })
            || (!matches!(expr, Expr::Identifier(_) | Expr::CompoundIdentifier(_))
                && self.grouping.is_grouped(expr));
        self.shields.push(shield);
        ControlFlow::Continue(())
    }

    fn post_visit_expr(&mut self, expr: &mut Expr) -> ControlFlow<()> {
        self.shields.pop();
        let shielded = self.shields.iter().any(|s| *s);
        if !shielded
            && matches!(expr, Expr::Identifier(_) | Expr::CompoundIdentifier(_))
            && !self.grouping.is_grouped(expr)
        {
            *expr = template("any_value(__a0)", vec![take(expr)]);
        }
        ControlFlow::Continue(())
    }
}

fn wrap(grouping: &Grouping, expr: &mut Expr) {
    let _ = expr.visit(&mut Wrap {
        grouping,
        shields: Vec::new(),
    });
}

fn contains_aggregate(expr: &Expr) -> bool {
    struct Find(bool);
    impl sqlparser::ast::Visitor for Find {
        type Break = ();
        fn pre_visit_expr(&mut self, expr: &Expr) -> ControlFlow<()> {
            if is_aggregate(expr) {
                self.0 = true;
            }
            ControlFlow::Continue(())
        }
    }
    let mut f = Find(false);
    let _ = sqlparser::ast::Visit::visit(expr, &mut f);
    f.0
}

fn tables_of(select: &Select) -> Vec<(String, String)> {
    select
        .from
        .iter()
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

struct Loose<'a> {
    schema: &'a SchemaCache,
}

impl Loose<'_> {
    /// Rewrites one SELECT; the grouping it used, for its query's ORDER BY.
    fn select<'s>(&'s self, select: &mut Select) -> Option<Grouping<'s>> {
        let grouped = match &select.group_by {
            GroupByExpr::Expressions(list, _) => list.clone(),
            GroupByExpr::All(_) => return None,
        };
        let aggregates = select.projection.iter().any(|i| match i {
            SelectItem::UnnamedExpr(e) | SelectItem::ExprWithAlias { expr: e, .. } => {
                contains_aggregate(e)
            }
            _ => false,
        }) || select.having.as_ref().is_some_and(contains_aggregate);
        if grouped.is_empty() && !aggregates {
            return None;
        }
        let grouping = Grouping {
            schema: self.schema,
            tables: tables_of(select),
            grouped,
        };
        let mut projection = Vec::new();
        for item in std::mem::take(&mut select.projection) {
            match item {
                SelectItem::Wildcard(_) if !grouping.grouped.is_empty() => {
                    for (label, table) in &grouping.tables {
                        for c in grouping.schema.columns(table).iter().flat_map(|c| c.iter()) {
                            let reference = if grouping.tables.len() == 1 {
                                Expr::Identifier(Ident::new(&c.name))
                            } else {
                                Expr::CompoundIdentifier(vec![
                                    Ident::new(label),
                                    Ident::new(&c.name),
                                ])
                            };
                            projection.push(SelectItem::UnnamedExpr(reference));
                        }
                    }
                }
                SelectItem::QualifiedWildcard(
                    SelectItemQualifiedWildcardKind::ObjectName(name),
                    _,
                ) if !grouping.grouped.is_empty()
                    && grouping
                        .tables
                        .iter()
                        .find(|(l, _)| l.eq_ignore_ascii_case(&object_name(&name)))
                        .is_some_and(|(_, t)| !grouping.key_grouped(t)) =>
                {
                    let label = object_name(&name);
                    let table = grouping
                        .tables
                        .iter()
                        .find(|(l, _)| l.eq_ignore_ascii_case(&label))
                        .map(|(_, t)| t.clone());
                    for c in table
                        .iter()
                        .flat_map(|t| grouping.schema.columns(t))
                        .flat_map(|c| c.to_vec())
                    {
                        projection.push(SelectItem::UnnamedExpr(Expr::CompoundIdentifier(vec![
                            Ident::new(&label),
                            Ident::new(&c.name),
                        ])));
                    }
                }
                other => projection.push(other),
            }
        }
        for item in &mut projection {
            match item {
                SelectItem::UnnamedExpr(expr) => {
                    let written = match &*expr {
                        Expr::Identifier(i) => Some(i.value.clone()),
                        Expr::CompoundIdentifier(parts) => parts.last().map(|p| p.value.clone()),
                        _ => None,
                    };
                    wrap(&grouping, expr);
                    if let Some(written) = written
                        && !matches!(expr, Expr::Identifier(_) | Expr::CompoundIdentifier(_))
                    {
                        // The wrapped column keeps the name MySQL gives it.
                        *item = SelectItem::ExprWithAlias {
                            expr: take(expr),
                            alias: Ident::new(written),
                        };
                    }
                }
                SelectItem::ExprWithAlias { expr, .. } => wrap(&grouping, expr),
                _ => {}
            }
        }
        // HAVING may name a select alias; PostgreSQL wants its expression.
        let aliases: Vec<(String, Expr)> = projection
            .iter()
            .filter_map(|i| match i {
                SelectItem::ExprWithAlias { expr, alias } => {
                    Some((alias.value.clone(), expr.clone()))
                }
                _ => None,
            })
            .collect();
        if let Some(having) = &mut select.having {
            struct Unalias<'a>(&'a [(String, Expr)]);
            impl VisitorMut for Unalias<'_> {
                type Break = ();
                fn post_visit_expr(&mut self, expr: &mut Expr) -> ControlFlow<()> {
                    if let Expr::Identifier(i) = expr
                        && let Some((_, e)) = self
                            .0
                            .iter()
                            .find(|(a, _)| a.eq_ignore_ascii_case(&i.value))
                    {
                        *expr = e.clone();
                    }
                    ControlFlow::Continue(())
                }
            }
            let _ = having.visit(&mut Unalias(&aliases));
            wrap(&grouping, having);
        }
        select.projection = projection;
        Some(grouping)
    }
}

/// `SELECT DISTINCT a … ORDER BY b` with `b` not selected: PostgreSQL refuses it, MySQL sorts by any row's `b`.
/// Grouping by the selected expressions gives the same rows, and the ORDER BY then reads `any_value(b)`.
fn distinct_as_group_by(select: &mut Select, order: Option<&sqlparser::ast::OrderBy>) {
    if !matches!(select.distinct, Some(sqlparser::ast::Distinct::Distinct)) {
        return;
    }
    let Some(sqlparser::ast::OrderBy {
        kind: sqlparser::ast::OrderByKind::Expressions(list),
        ..
    }) = order
    else {
        return;
    };
    let mut selected: Vec<Expr> = Vec::new();
    let mut names: Vec<String> = Vec::new();
    for item in &select.projection {
        match item {
            SelectItem::UnnamedExpr(e) => selected.push(e.clone()),
            SelectItem::ExprWithAlias { expr, alias } => {
                selected.push(expr.clone());
                names.push(alias.value.to_ascii_lowercase());
            }
            _ => return,
        }
    }
    let plain = |e: &Expr| match e {
        Expr::Nested(inner) => (**inner).clone(),
        other => other.clone(),
    };
    let text = |e: &Expr| plain(e).to_string().to_ascii_lowercase();
    let listed = list.iter().all(|o| {
        let t = text(&o.expr);
        selected.iter().any(|s| text(s) == t) || names.contains(&t)
    });
    if listed {
        return;
    }
    // Aggregates are what a group computes, never what it is grouped by.
    let grouped: Vec<Expr> = selected
        .iter()
        .filter(|e| !contains_aggregate(e))
        .map(plain)
        .collect();
    if grouped.is_empty() {
        return;
    }
    select.distinct = None;
    select.group_by = GroupByExpr::Expressions(grouped, Vec::new());
}

impl VisitorMut for Loose<'_> {
    type Break = ();

    fn pre_visit_query(&mut self, query: &mut Query) -> ControlFlow<()> {
        if let SetExpr::Select(select) = query.body.as_mut() {
            distinct_as_group_by(select, query.order_by.as_ref());
            let aliases: Vec<String> = select
                .projection
                .iter()
                .filter_map(|i| match i {
                    SelectItem::ExprWithAlias { alias, .. } => Some(alias.value.clone()),
                    _ => None,
                })
                .collect();
            if let Some(grouping) = self.select(select)
                && let Some(order) = &mut query.order_by
                && let sqlparser::ast::OrderByKind::Expressions(list) = &mut order.kind
            {
                for o in list.iter_mut() {
                    let is_alias = matches!(&o.expr, Expr::Identifier(i) if aliases.iter().any(|a| a.eq_ignore_ascii_case(&i.value)));
                    if !is_alias {
                        wrap(&grouping, &mut o.expr);
                    }
                }
            }
        }
        ControlFlow::Continue(())
    }
}

impl RewritePass for PgLooseGroupBy {
    fn name(&self) -> &'static str {
        "pg_loose_group_by"
    }

    fn rewrite(&self, stmt: &mut Statement, _cx: &PassContext) -> Result<Rewrite, DbError> {
        let _ = stmt.tree_mut().visit(&mut Loose {
            schema: &self.schema,
        });
        Ok(Rewrite::Continue)
    }
}
