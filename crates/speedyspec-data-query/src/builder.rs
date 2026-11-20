//! The query builder (Builder): an Eloquent-style API that changes a syntax tree.
//!
//! Values become `?` placeholders. While building, each placeholder carries a private numbered name so modifiers
//! may be called in any order; `build` renames them `?` in the tree's textual order and orders the params to match.

use std::collections::HashMap;
use std::ops::ControlFlow;

use speedyspec_data::{DbError, DbValue, DialectKind, Statement};
use sqlparser::ast::{
    self, Assignment, AssignmentTarget, Expr, Ident, ObjectName, OnInsert, OrderByExpr,
    OrderByKind, OrderByOptions, SelectItem, SetExpr, Value, visit_expressions_mut,
};
use sqlparser::dialect::MySqlDialect;
use sqlparser::keywords::ALL_KEYWORDS;
use sqlparser::parser::Parser;

const MARK: &str = "?speedyspec_param_";

/// Sort direction for `order_by`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Order {
    Asc,
    Desc,
}

/// A statement under construction, with the first error a modifier met.
#[derive(Debug, Clone)]
pub struct Query {
    pub(crate) tree: ast::Statement,
    pub(crate) params: HashMap<usize, DbValue>,
    pub(crate) next: usize,
    pub(crate) error: Option<DbError>,
}

impl Query {
    /// `SELECT * FROM <table>`.
    pub fn table(table: &str) -> Query {
        let mut q = Query::skeleton("SELECT * FROM t");
        if let Some(select) = q.select_mut() {
            if let ast::TableFactor::Table { name, .. } = &mut select.from[0].relation {
                *name = object_name(table);
            }
        }
        q
    }

    /// `INSERT INTO <table> (<columns>)`; add rows with `values`.
    pub fn insert_into(table: &str, columns: &[&str]) -> Query {
        Query::insert(table, columns, false)
    }

    /// `REPLACE INTO <table> (<columns>)`; add rows with `values`.
    pub fn replace_into(table: &str, columns: &[&str]) -> Query {
        Query::insert(table, columns, true)
    }

    /// `UPDATE <table>`; add assignments with `set`.
    pub fn update(table: &str) -> Query {
        let mut q = Query::skeleton("UPDATE t SET a = 1");
        if let ast::Statement::Update(update) = &mut q.tree {
            update.assignments.clear();
            if let ast::TableFactor::Table { name, .. } = &mut update.table.relation {
                *name = object_name(table);
            }
        }
        q
    }

    /// `DELETE FROM <table>`.
    pub fn delete_from(table: &str) -> Query {
        let mut q = Query::skeleton("DELETE FROM t");
        if let ast::Statement::Delete(delete) = &mut q.tree {
            if let ast::FromTable::WithFromKeyword(from) = &mut delete.from {
                if let ast::TableFactor::Table { name, .. } = &mut from[0].relation {
                    *name = object_name(table);
                }
            }
        }
        q
    }

    /// Wraps a parsed statement; its `?` placeholders keep their params.
    pub fn from_statement(stmt: Statement) -> Query {
        let (mut tree, params) = stmt.into_parts();
        let mut params = params.into_iter();
        let mut map = HashMap::new();
        let mut next = 0;
        let mut short = false;
        let _ = visit_expressions_mut(&mut tree, |expr| {
            if let Expr::Value(v) = expr {
                if matches!(&v.value, Value::Placeholder(p) if p == "?") {
                    match params.next() {
                        Some(value) => {
                            v.value = Value::Placeholder(format!("{MARK}{next}"));
                            map.insert(next, value);
                            next += 1;
                        }
                        None => short = true,
                    }
                }
            }
            ControlFlow::<()>::Continue(())
        });
        let error = (short || params.next().is_some()).then(|| {
            DbError::Config("the statement's placeholders and params differ in number".into())
        });
        Query {
            tree,
            params: map,
            next,
            error,
        }
    }

    /// Replaces the projection of a `SELECT`.
    pub fn select(&mut self, columns: &[&str]) -> &mut Query {
        match self.select_mut() {
            Some(select) => {
                select.projection = columns
                    .iter()
                    .map(|c| SelectItem::UnnamedExpr(column(c)))
                    .collect()
            }
            None => self.unsupported("SELECT columns outside SELECT"),
        }
        self
    }

    /// Adds `<column> = ?` to the `WHERE` clause with `AND`.
    pub fn where_eq(&mut self, col: &str, value: impl Into<DbValue>) -> &mut Query {
        let rhs = self.param(value.into());
        self.and_condition(Expr::BinaryOp {
            left: Box::new(column(col)),
            op: ast::BinaryOperator::Eq,
            right: Box::new(rhs),
        });
        self
    }

    /// Adds `<column> IN (?, ..)` to the `WHERE` clause with `AND`.
    pub fn where_in(
        &mut self,
        col: &str,
        values: impl IntoIterator<Item = impl Into<DbValue>>,
    ) -> &mut Query {
        let list = values.into_iter().map(|v| self.param(v.into())).collect();
        self.and_condition(Expr::In {
            expr: Box::new(column(col)),
            target: ast::InTarget::List(list),
            negated: false,
            global: false,
        });
        self
    }

    /// Appends an `ORDER BY` term.
    pub fn order_by(&mut self, col: &str, order: Order) -> &mut Query {
        let term = OrderByExpr {
            expr: column(col),
            options: OrderByOptions {
                sort: Some(match order {
                    Order::Asc => ast::OrderBySort::Asc,
                    Order::Desc => ast::OrderBySort::Desc,
                }),
                nulls_first: None,
            },
            with_fill: None,
        };
        self.push_order(term);
        self
    }

    /// Sets `LIMIT`.
    pub fn limit(&mut self, n: u64) -> &mut Query {
        self.set_limit(Some(number(n)), None);
        self
    }

    /// Sets `OFFSET` (on `SELECT` only).
    pub fn offset(&mut self, n: u64) -> &mut Query {
        self.set_limit(None, Some(number(n)));
        self
    }

    /// Adds one row to an `INSERT` or `REPLACE`; call again for more rows.
    pub fn values(&mut self, row: Vec<DbValue>) -> &mut Query {
        let exprs: Vec<Expr> = row.into_iter().map(|v| self.param(v)).collect();
        match &mut self.tree {
            ast::Statement::Insert(insert) => {
                match insert.source.as_deref_mut().map(|q| &mut *q.body) {
                    Some(SetExpr::Values(values)) => {
                        values.rows.push(ast::Parens::with_empty_span(exprs))
                    }
                    _ => self.unsupported("VALUES on INSERT ... SELECT"),
                }
            }
            _ => self.unsupported("VALUES outside INSERT"),
        }
        self
    }

    /// `ON DUPLICATE KEY UPDATE col = VALUES(col), ..`, as core's `add_option` writes it.
    pub fn on_duplicate_update(&mut self, columns: &[&str]) -> &mut Query {
        match &mut self.tree {
            ast::Statement::Insert(insert) => {
                let assignments = columns
                    .iter()
                    .map(|c| Assignment {
                        target: AssignmentTarget::ColumnName(object_name(c)),
                        value: values_call(c),
                    })
                    .collect();
                insert.on = Some(OnInsert::DuplicateKeyUpdate(assignments));
            }
            _ => self.unsupported("ON DUPLICATE KEY UPDATE outside INSERT"),
        }
        self
    }

    /// Adds `<column> = ?` to an `UPDATE`'s `SET`.
    pub fn set(&mut self, col: &str, value: impl Into<DbValue>) -> &mut Query {
        let value = value.into();
        if !matches!(self.tree, ast::Statement::Update(_)) {
            let what = match self.tree {
                ast::Statement::Query(_) => "SET on SELECT",
                _ => "SET outside UPDATE",
            };
            self.unsupported(what);
            return self;
        }
        let rhs = self.param(value);
        if let ast::Statement::Update(update) = &mut self.tree {
            update.assignments.push(Assignment {
                target: AssignmentTarget::ColumnName(object_name(col)),
                value: rhs,
            });
        }
        self
    }

    /// The statement, or the first error a modifier met.
    pub fn build(&self) -> Result<Statement, DbError> {
        if let Some(error) = &self.error {
            return Err(error.clone());
        }
        let mut tree = self.tree.clone();
        let mut params = Vec::with_capacity(self.params.len());
        let mut missing = false;
        let _ = visit_expressions_mut(&mut tree, |expr| {
            if let Expr::Value(v) = expr {
                if let Value::Placeholder(p) = &v.value {
                    if let Some(n) = p.strip_prefix(MARK).and_then(|n| n.parse::<usize>().ok()) {
                        match self.params.get(&n) {
                            Some(value) => params.push(value.clone()),
                            None => missing = true,
                        }
                        v.value = Value::Placeholder("?".into());
                    }
                }
            }
            ControlFlow::<()>::Continue(())
        });
        if missing {
            return Err(DbError::Config("a placeholder lost its param".into()));
        }
        Ok(Statement::new(tree, params))
    }

    // ---- internals shared with partials ----

    fn skeleton(sql: &str) -> Query {
        let tree = Parser::parse_sql(&MySqlDialect {}, sql)
            .expect("builder skeletons are valid SQL")
            .remove(0);
        Query {
            tree,
            params: HashMap::new(),
            next: 0,
            error: None,
        }
    }

    fn insert(table: &str, columns: &[&str], replace: bool) -> Query {
        let mut q = Query::skeleton("INSERT INTO t (a) VALUES (1)");
        if let ast::Statement::Insert(insert) = &mut q.tree {
            insert.table = ast::TableObject::TableName(object_name(table));
            insert.columns = columns.iter().map(|c| object_name(c)).collect();
            insert.replace_into = replace;
            if let Some(SetExpr::Values(values)) =
                insert.source.as_deref_mut().map(|q| &mut *q.body)
            {
                values.rows.clear();
            }
        }
        q
    }

    pub(crate) fn unsupported(&mut self, construct: &str) {
        if self.error.is_none() {
            self.error = Some(DbError::Unsupported {
                construct: construct.into(),
                dialect: DialectKind::MySql,
            });
        }
    }

    fn param(&mut self, value: DbValue) -> Expr {
        let n = self.next;
        self.next += 1;
        self.params.insert(n, value);
        Expr::Value(Value::Placeholder(format!("{MARK}{n}")).with_empty_span())
    }

    pub(crate) fn select_mut(&mut self) -> Option<&mut ast::Select> {
        match &mut self.tree {
            ast::Statement::Query(query) => match &mut *query.body {
                SetExpr::Select(select) => Some(select),
                _ => None,
            },
            _ => None,
        }
    }

    pub(crate) fn selection_mut(&mut self) -> Option<&mut Option<Expr>> {
        match &mut self.tree {
            ast::Statement::Query(query) => match &mut *query.body {
                SetExpr::Select(select) => Some(&mut select.selection),
                _ => None,
            },
            ast::Statement::Update(update) => Some(&mut update.selection),
            ast::Statement::Delete(delete) => Some(&mut delete.selection),
            _ => None,
        }
    }

    pub(crate) fn and_condition(&mut self, condition: Expr) {
        match self.selection_mut() {
            Some(selection) => {
                *selection = Some(match selection.take() {
                    Some(existing) => crate::partial::and(existing, condition),
                    None => condition,
                })
            }
            None => self.unsupported("WHERE outside SELECT, UPDATE and DELETE"),
        }
    }

    pub(crate) fn push_order(&mut self, term: OrderByExpr) {
        match &mut self.tree {
            ast::Statement::Query(query) => {
                let order = query.order_by.get_or_insert_with(|| ast::OrderBy {
                    kind: OrderByKind::Expressions(Vec::new()),
                    interpolate: None,
                });
                match &mut order.kind {
                    OrderByKind::Expressions(terms) => terms.push(term),
                    OrderByKind::All(_) => order.kind = OrderByKind::Expressions(vec![term]),
                }
            }
            ast::Statement::Update(update) => update.order_by.push(term),
            ast::Statement::Delete(delete) => delete.order_by.push(term),
            _ => self.unsupported("ORDER BY outside SELECT, UPDATE and DELETE"),
        }
    }

    fn set_limit(&mut self, limit: Option<Expr>, offset: Option<Expr>) {
        match &mut self.tree {
            ast::Statement::Query(query) => {
                let (mut cur_limit, mut cur_offset) = match query.limit_clause.take() {
                    Some(ast::LimitClause::LimitOffset { limit, offset, .. }) => {
                        (limit, offset.map(|o| o.value))
                    }
                    Some(ast::LimitClause::OffsetCommaLimit { offset, limit }) => {
                        (Some(limit), Some(offset))
                    }
                    None => (None, None),
                };
                if limit.is_some() {
                    cur_limit = limit;
                }
                if offset.is_some() {
                    cur_offset = offset;
                }
                query.limit_clause = Some(ast::LimitClause::LimitOffset {
                    limit: cur_limit,
                    offset: cur_offset.map(|value| ast::Offset {
                        value,
                        rows: ast::OffsetRows::None,
                    }),
                    limit_by: Vec::new(),
                });
            }
            ast::Statement::Update(update) if offset.is_none() => update.limit = limit,
            ast::Statement::Delete(delete) if offset.is_none() => delete.limit = limit,
            _ if offset.is_some() => self.unsupported("OFFSET outside SELECT"),
            _ => self.unsupported("LIMIT outside SELECT, UPDATE and DELETE"),
        }
    }
}

/// An identifier, backquoted when it is a keyword or not a plain name.
pub(crate) fn ident(name: &str) -> Ident {
    let plain = name
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$');
    let keyword = ALL_KEYWORDS
        .binary_search(&name.to_ascii_uppercase().as_str())
        .is_ok();
    if plain && !keyword {
        Ident::new(name)
    } else {
        Ident::with_quote('`', name)
    }
}

pub(crate) fn object_name(name: &str) -> ObjectName {
    ObjectName::from(name.split('.').map(ident).collect::<Vec<_>>())
}

pub(crate) fn column(name: &str) -> Expr {
    let mut parts: Vec<Ident> = name.split('.').map(ident).collect();
    if parts.len() == 1 {
        Expr::Identifier(parts.remove(0))
    } else {
        Expr::CompoundIdentifier(parts)
    }
}

fn number(n: u64) -> Expr {
    Expr::Value(Value::Number(n.to_string(), false).with_empty_span())
}

fn values_call(col: &str) -> Expr {
    let mut expr = Parser::new(&MySqlDialect {})
        .try_with_sql("VALUES(x)")
        .and_then(|mut p| p.parse_expr())
        .expect("VALUES(x) is a valid expression");
    let _ = visit_expressions_mut(&mut expr, |e| {
        if matches!(e, Expr::Identifier(i) if i.value == "x") {
            *e = column(col);
        }
        ControlFlow::<()>::Continue(())
    });
    expr
}
