//! Partials: SQL fragments parsed on their own and placed into a builder's tree.

use speedyspec_data::DbError;
use sqlparser::ast::{
    self, Expr, GroupByExpr, Join, OrderByExpr, OrderByKind, SelectItem, SetExpr,
};
use sqlparser::dialect::MySqlDialect;
use sqlparser::parser::Parser;

use crate::Query;
use crate::parse::{parse_error, parse_one};

const HOST: &str = "speedyspec_partial";

/// The clauses a partial can replace or remove.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Clause {
    Projection,
    Joins,
    Where,
    GroupBy,
    Having,
    OrderBy,
    Limit,
}

#[derive(Debug, Clone, PartialEq)]
enum Fragment {
    Expr(Box<Expr>),
    OrderBy(Vec<OrderByExpr>),
    GroupBy(Vec<Expr>),
    Projection(Vec<SelectItem>),
    Joins(Vec<Join>),
}

/// A parsed SQL fragment: an expression, an `ORDER BY` or `GROUP BY` list, a projection, or joins.
#[derive(Debug, Clone, PartialEq)]
pub struct Partial(Fragment);

impl Partial {
    /// A condition or other expression, e.g. `post_type = 'page'`.
    pub fn expr(sql: &str) -> Result<Partial, DbError> {
        let mut parser = Parser::new(&MySqlDialect {})
            .try_with_sql(sql)
            .map_err(parse_error)?;
        let expr = parser.parse_expr().map_err(parse_error)?;
        if parser.peek_token().token != sqlparser::tokenizer::Token::EOF {
            return Err(fragment_error(sql));
        }
        Ok(Partial(Fragment::Expr(Box::new(expr))))
    }

    /// The terms of an `ORDER BY`, e.g. `post_date DESC`.
    pub fn order_by(sql: &str) -> Result<Partial, DbError> {
        let mut query = host(&format!("SELECT 1 FROM {HOST} ORDER BY {sql}"), sql)?;
        let terms = match query.order_by.take().map(|o| o.kind) {
            Some(OrderByKind::Expressions(terms)) => terms,
            _ => return Err(fragment_error(sql)),
        };
        only_host(&query, sql)?;
        Ok(Partial(Fragment::OrderBy(terms)))
    }

    /// The terms of a `GROUP BY`, e.g. `p.ID`.
    pub fn group_by(sql: &str) -> Result<Partial, DbError> {
        let mut query = host(&format!("SELECT 1 FROM {HOST} GROUP BY {sql}"), sql)?;
        let select = host_select(&mut query, sql)?;
        let terms = match std::mem::replace(
            &mut select.group_by,
            GroupByExpr::Expressions(vec![], vec![]),
        ) {
            GroupByExpr::Expressions(terms, modifiers) if modifiers.is_empty() => terms,
            _ => return Err(fragment_error(sql)),
        };
        only_host(&query, sql)?;
        Ok(Partial(Fragment::GroupBy(terms)))
    }

    /// A projection list, e.g. `p.ID, COUNT(*) AS n`.
    pub fn projection(sql: &str) -> Result<Partial, DbError> {
        let mut query = host(&format!("SELECT {sql} FROM {HOST}"), sql)?;
        let select = host_select(&mut query, sql)?;
        let items = std::mem::replace(&mut select.projection, one_projection());
        only_host(&query, sql)?;
        Ok(Partial(Fragment::Projection(items)))
    }

    /// One or more joins, e.g. `INNER JOIN wp_postmeta m ON m.post_id = p.ID`.
    pub fn join(sql: &str) -> Result<Partial, DbError> {
        let mut query = host(&format!("SELECT 1 FROM {HOST} {sql}"), sql)?;
        let select = host_select(&mut query, sql)?;
        if select.from.len() != 1 {
            return Err(fragment_error(sql));
        }
        let joins = std::mem::take(&mut select.from[0].joins);
        if joins.is_empty() {
            return Err(fragment_error(sql));
        }
        only_host(&query, sql)?;
        Ok(Partial(Fragment::Joins(joins)))
    }
}

fn fragment_error(sql: &str) -> DbError {
    DbError::Parse {
        message: format!("not a single SQL fragment of the expected kind: {sql}"),
        line: 0,
        column: 0,
    }
}

fn host(sql: &str, fragment: &str) -> Result<Box<ast::Query>, DbError> {
    match parse_one(sql)
        .map_err(|e| match e {
            DbError::Parse { message, .. } if message == "multiple statements in one query" => {
                fragment_error(fragment)
            }
            other => other,
        })?
        .into_parts()
        .0
    {
        ast::Statement::Query(query) => Ok(query),
        _ => Err(fragment_error(fragment)),
    }
}

fn host_select<'a>(query: &'a mut ast::Query, sql: &str) -> Result<&'a mut ast::Select, DbError> {
    match &mut *query.body {
        SetExpr::Select(select) => Ok(select),
        _ => Err(fragment_error(sql)),
    }
}

fn one_projection() -> Vec<SelectItem> {
    vec![SelectItem::UnnamedExpr(Expr::Value(
        ast::Value::Number("1".into(), false).with_empty_span(),
    ))]
}

/// After the fragment is lifted out, what is left must be exactly `SELECT 1 FROM <host>`.
fn only_host(query: &ast::Query, sql: &str) -> Result<(), DbError> {
    let empty = host(&format!("SELECT 1 FROM {HOST}"), sql)?;
    if *query == *empty {
        Ok(())
    } else {
        Err(fragment_error(sql))
    }
}

/// True when the expression's text holds an `OR` or `XOR` outside parentheses. The tree's shape cannot be trusted
/// for this: sqlparser binds `XOR` more tightly than `=`, while MySQL binds it more loosely than `AND`, and the text
/// is what MySQL reads.
fn has_loose_operator(expr: &Expr) -> bool {
    match expr {
        Expr::BinaryOp {
            op: ast::BinaryOperator::Or | ast::BinaryOperator::Xor,
            ..
        } => true,
        Expr::BinaryOp { left, right, .. } => has_loose_operator(left) || has_loose_operator(right),
        Expr::UnaryOp { expr, .. } => has_loose_operator(expr),
        _ => false,
    }
}

/// `OR` and `XOR` bind more loosely than `AND` in MySQL, so a side of an added `AND` holding one is parenthesised.
fn wrap_or(expr: Expr) -> Expr {
    if has_loose_operator(&expr) {
        Expr::Nested(Box::new(expr))
    } else {
        expr
    }
}

/// `left AND right`, parenthesising either side that holds an unparenthesised `OR` or `XOR`.
pub(crate) fn and(left: Expr, right: Expr) -> Expr {
    Expr::BinaryOp {
        left: Box::new(wrap_or(left)),
        op: ast::BinaryOperator::And,
        right: Box::new(wrap_or(right)),
    }
}

impl Query {
    /// Parses one statement into a builder.
    pub fn parse(sql: &str) -> Result<Query, DbError> {
        Ok(Query::from_statement(parse_one(sql)?))
    }

    /// Adds a condition with `AND`.
    pub fn and_where(&mut self, partial: Partial) -> &mut Query {
        match partial.0 {
            Fragment::Expr(expr) => self.and_condition(*expr),
            _ => self.unsupported("a non-expression partial in WHERE"),
        }
        self
    }

    /// Adds a condition with `OR`, parenthesising the existing condition.
    pub fn or_where(&mut self, partial: Partial) -> &mut Query {
        let Fragment::Expr(expr) = partial.0 else {
            self.unsupported("a non-expression partial in WHERE");
            return self;
        };
        match self.selection_mut() {
            Some(selection) => {
                *selection = Some(match selection.take() {
                    Some(existing) => Expr::BinaryOp {
                        left: Box::new(Expr::Nested(Box::new(existing))),
                        op: ast::BinaryOperator::Or,
                        right: expr,
                    },
                    None => *expr,
                })
            }
            None => self.unsupported("WHERE outside SELECT, UPDATE and DELETE"),
        }
        self
    }

    /// Appends joins to a `SELECT`'s first table.
    pub fn join(&mut self, partial: Partial) -> &mut Query {
        let Fragment::Joins(joins) = partial.0 else {
            self.unsupported("a non-join partial in JOIN");
            return self;
        };
        match self.select_mut().and_then(|s| s.from.first_mut()) {
            Some(from) => from.joins.extend(joins),
            None => self.unsupported("JOIN outside SELECT ... FROM"),
        }
        self
    }

    /// Replaces a clause with a partial of the matching kind.
    pub fn replace(&mut self, clause: Clause, partial: Partial) -> &mut Query {
        self.remove(clause);
        if self.error.is_some() {
            return self;
        }
        match (clause, partial.0) {
            (Clause::Where, Fragment::Expr(expr)) => self.and_condition(*expr),
            (Clause::Projection, Fragment::Projection(items)) => match self.select_mut() {
                Some(select) => select.projection = items,
                None => self.unsupported("SELECT columns outside SELECT"),
            },
            (Clause::Joins, Fragment::Joins(joins)) => {
                if let Some(from) = self.select_mut().and_then(|s| s.from.first_mut()) {
                    from.joins = joins;
                }
            }
            (Clause::GroupBy, Fragment::GroupBy(terms)) => {
                if let Some(select) = self.select_mut() {
                    select.group_by = GroupByExpr::Expressions(terms, vec![]);
                }
            }
            (Clause::Having, Fragment::Expr(expr)) => {
                if let Some(select) = self.select_mut() {
                    select.having = Some(*expr);
                }
            }
            (Clause::OrderBy, Fragment::OrderBy(terms)) => {
                for term in terms {
                    self.push_order(term);
                }
            }
            (Clause::Limit, Fragment::Expr(expr)) => match &mut self.tree {
                ast::Statement::Query(query) => {
                    query.limit_clause = Some(ast::LimitClause::LimitOffset {
                        limit: Some(*expr),
                        offset: None,
                        limit_by: vec![],
                    })
                }
                ast::Statement::Update(update) => update.limit = Some(*expr),
                ast::Statement::Delete(delete) => delete.limit = Some(*expr),
                _ => self.unsupported("LIMIT outside SELECT, UPDATE and DELETE"),
            },
            (clause, _) => self.unsupported(&format!("a partial of another kind for {clause:?}")),
        }
        self
    }

    /// Removes a clause.
    pub fn remove(&mut self, clause: Clause) -> &mut Query {
        let ok = match (&mut self.tree, clause) {
            (ast::Statement::Query(query), Clause::OrderBy) => {
                query.order_by = None;
                true
            }
            (ast::Statement::Query(query), Clause::Limit) => {
                query.limit_clause = None;
                true
            }
            (ast::Statement::Query(query), _) => match &mut *query.body {
                SetExpr::Select(select) => match clause {
                    Clause::Projection => {
                        select.projection = vec![SelectItem::Wildcard(Default::default())];
                        true
                    }
                    Clause::Joins => select.from.first_mut().map(|f| f.joins.clear()).is_some(),
                    Clause::Where => {
                        select.selection = None;
                        true
                    }
                    Clause::GroupBy => {
                        select.group_by = GroupByExpr::Expressions(vec![], vec![]);
                        true
                    }
                    Clause::Having => {
                        select.having = None;
                        true
                    }
                    Clause::OrderBy | Clause::Limit => unreachable!("handled above"),
                },
                _ => false,
            },
            (ast::Statement::Update(update), Clause::Where) => {
                update.selection = None;
                true
            }
            (ast::Statement::Update(update), Clause::OrderBy) => {
                update.order_by.clear();
                true
            }
            (ast::Statement::Update(update), Clause::Limit) => {
                update.limit = None;
                true
            }
            (ast::Statement::Delete(delete), Clause::Where) => {
                delete.selection = None;
                true
            }
            (ast::Statement::Delete(delete), Clause::OrderBy) => {
                delete.order_by.clear();
                true
            }
            (ast::Statement::Delete(delete), Clause::Limit) => {
                delete.limit = None;
                true
            }
            _ => false,
        };
        if !ok {
            self.unsupported(&format!("{clause:?} on this statement"));
        }
        self
    }
}
