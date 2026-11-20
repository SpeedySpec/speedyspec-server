//! `PgExpressions`: MySQL's functions, truthiness, integer division, casts and text-versus-number comparisons as
//! PostgreSQL expressions.

use std::ops::ControlFlow;
use std::rc::Rc;

use speedyspec_data::{DbError, DbValue, Statement};
use speedyspec_data_query::{PassContext, Rewrite, RewritePass};
use speedyspec_data_translate::TypeFamily;
use sqlparser::ast::{
    BinaryOperator, CastKind, DataType, Expr, Ident, InTarget, JoinConstraint, JoinOperator,
    ObjectName, Select, Statement as Ast, UnaryOperator, Value, VisitMut, VisitorMut,
};

use super::functions::{rewrite_call, template, unsupported};
use crate::schema::{SchemaCache, tables_in};

pub(crate) struct PgExpressions {
    schema: Rc<SchemaCache>,
}

impl PgExpressions {
    pub(crate) fn new(schema: Rc<SchemaCache>) -> PgExpressions {
        PgExpressions { schema }
    }
}

fn take(expr: &mut Expr) -> Expr {
    std::mem::replace(expr, Expr::value(Value::Null))
}

fn atomic(expr: &Expr) -> bool {
    matches!(
        expr,
        Expr::Identifier(_)
            | Expr::CompoundIdentifier(_)
            | Expr::Value(_)
            | Expr::Function(_)
            | Expr::Nested(_)
    )
}

/// `x::type`, the operand parenthesised unless it is atomic.
pub(crate) fn cast(expr: Expr, data_type: DataType) -> Expr {
    let expr = if atomic(&expr) {
        expr
    } else {
        Expr::Nested(Box::new(expr))
    };
    Expr::Cast {
        kind: CastKind::DoubleColon,
        expr: Box::new(expr),
        data_type,
        format: None,
    }
}

fn is_boolean(expr: &Expr) -> bool {
    use BinaryOperator as B;
    match expr {
        Expr::BinaryOp { op, .. } => matches!(
            op,
            B::Eq
                | B::NotEq
                | B::Lt
                | B::LtEq
                | B::Gt
                | B::GtEq
                | B::And
                | B::Or
                | B::Xor
                | B::PGRegexMatch
                | B::PGRegexIMatch
                | B::PGRegexNotMatch
                | B::PGRegexNotIMatch
        ),
        Expr::UnaryOp {
            op: UnaryOperator::Not,
            ..
        } => true,
        Expr::Nested(inner) => is_boolean(inner),
        Expr::Value(v) => matches!(v.value, Value::Boolean(_)),
        Expr::IsNull(_)
        | Expr::IsNotNull(_)
        | Expr::IsTrue(_)
        | Expr::IsFalse(_)
        | Expr::IsNotTrue(_)
        | Expr::IsNotFalse(_)
        | Expr::IsDistinctFrom(..)
        | Expr::IsNotDistinctFrom(..)
        | Expr::Like { .. }
        | Expr::ILike { .. }
        | Expr::RLike { .. }
        | Expr::SimilarTo { .. }
        | Expr::In { .. }
        | Expr::Between { .. }
        | Expr::Exists { .. } => true,
        _ => false,
    }
}

/// A MySQL condition as a PostgreSQL boolean: a number is true when it is not 0.
pub(crate) fn truthy(expr: Expr) -> Expr {
    match expr {
        Expr::BinaryOp {
            left,
            op: op @ (BinaryOperator::And | BinaryOperator::Or | BinaryOperator::Xor),
            right,
        } => Expr::BinaryOp {
            left: Box::new(truthy(*left)),
            op,
            right: Box::new(truthy(*right)),
        },
        Expr::UnaryOp {
            op: UnaryOperator::Not,
            expr,
        } => Expr::UnaryOp {
            op: UnaryOperator::Not,
            expr: Box::new(truthy(*expr)),
        },
        Expr::Nested(inner) if !is_boolean(&inner) => Expr::Nested(Box::new(truthy(*inner))),
        e if is_boolean(&e) => e,
        Expr::Value(v) if matches!(&v.value, Value::Number(..)) => {
            let zero =
                matches!(&v.value, Value::Number(n, _) if n.parse::<f64>().is_ok_and(|n| n == 0.0));
            Expr::value(Value::Boolean(!zero))
        }
        e => Expr::BinaryOp {
            left: Box::new(cast(
                e,
                DataType::Numeric(sqlparser::ast::ExactNumberInfo::None),
            )),
            op: BinaryOperator::NotEq,
            right: Box::new(Expr::value(Value::Number("0".into(), false))),
        },
    }
}

fn call(name: &str, arg: Expr) -> Expr {
    template(&format!("{name}(__a0)"), vec![arg])
}

struct Exprs<'a> {
    schema: &'a SchemaCache,
    tables: Vec<(String, String)>,
    params: &'a [DbValue],
    error: Option<DbError>,
}

impl Exprs<'_> {
    fn is_text_column(&self, expr: &Expr) -> bool {
        self.schema
            .type_of(&self.tables, expr)
            .is_some_and(|t| matches!(t.family(), TypeFamily::Text | TypeFamily::Enum))
    }

    fn is_number(&self, expr: &Expr) -> bool {
        match expr {
            Expr::Value(v) => match &v.value {
                Value::Number(..) => true,
                Value::Placeholder(p) => p
                    .strip_prefix('?')
                    .and_then(|n| n.parse::<usize>().ok())
                    .and_then(|n| self.params.get(n.wrapping_sub(1)))
                    .is_some_and(|v| {
                        matches!(v, DbValue::Int(_) | DbValue::UInt(_) | DbValue::Float(_))
                    }),
                _ => false,
            },
            Expr::UnaryOp {
                op: UnaryOperator::Minus,
                expr,
            } => self.is_number(expr),
            _ => false,
        }
    }

    fn double(&self, expr: &mut Expr) {
        let inner = take(expr);
        *expr = call("speedyspec_double", inner);
    }
}

fn is_binary_cast(expr: &Expr) -> bool {
    matches!(
        expr,
        Expr::Cast {
            data_type: DataType::Binary(None),
            ..
        }
    )
}

/// `BINARY x` (or `CAST(x AS BINARY)`) compared or matched: a byte-order comparison of the text.
fn binary_operand(expr: &mut Expr) {
    if is_binary_cast(expr)
        && let Expr::Cast { expr: inner, .. } = take(expr)
    {
        *expr = Expr::Collate {
            expr: inner,
            collation: ObjectName::from(vec![Ident::new("C")]),
        };
    }
}

impl VisitorMut for Exprs<'_> {
    type Break = ();

    fn pre_visit_select(&mut self, select: &mut Select) -> ControlFlow<()> {
        if let Some(w) = select.selection.take() {
            select.selection = Some(truthy(w));
        }
        if let Some(h) = select.having.take() {
            select.having = Some(truthy(h));
        }
        for from in &mut select.from {
            for join in &mut from.joins {
                if let JoinOperator::Join(JoinConstraint::On(on))
                | JoinOperator::Inner(JoinConstraint::On(on))
                | JoinOperator::Left(JoinConstraint::On(on))
                | JoinOperator::LeftOuter(JoinConstraint::On(on))
                | JoinOperator::Right(JoinConstraint::On(on))
                | JoinOperator::RightOuter(JoinConstraint::On(on)) = &mut join.join_operator
                {
                    *on = truthy(take(on));
                }
            }
        }
        ControlFlow::Continue(())
    }

    fn pre_visit_expr(&mut self, expr: &mut Expr) -> ControlFlow<()> {
        use BinaryOperator as B;
        match expr {
            Expr::BinaryOp {
                left,
                op: B::Eq | B::NotEq | B::Lt | B::LtEq | B::Gt | B::GtEq,
                right,
            } => {
                binary_operand(left);
                binary_operand(right);
            }
            Expr::Like { expr, pattern, .. } | Expr::RLike { expr, pattern, .. } => {
                binary_operand(expr);
                binary_operand(pattern);
            }
            Expr::MatchAgainst { .. } => {
                self.error = Some(unsupported("MATCH ... AGAINST"));
                return ControlFlow::Break(());
            }
            _ => {}
        }
        ControlFlow::Continue(())
    }

    fn post_visit_expr(&mut self, expr: &mut Expr) -> ControlFlow<()> {
        use BinaryOperator as B;
        match expr {
            Expr::Function(f) => match rewrite_call(f, &truthy) {
                Ok(Some(e)) => *expr = e,
                Ok(None) => {}
                Err(e) => {
                    self.error = Some(e);
                    return ControlFlow::Break(());
                }
            },
            Expr::BinaryOp {
                left,
                op: B::MyIntegerDivide,
                right,
            } => {
                *expr = template("trunc(__a0 / __a1)::bigint", vec![take(left), take(right)]);
            }
            Expr::Case { conditions, .. } => {
                for when in conditions.iter_mut() {
                    when.condition = truthy(take(&mut when.condition));
                }
            }
            Expr::Cast {
                data_type,
                expr: inner,
                ..
            } => {
                let text = |e: Expr| cast(e, DataType::Text);
                let replacement = match data_type.clone() {
                    DataType::BigInt(_)
                    | DataType::Signed
                    | DataType::Unsigned
                    | DataType::SignedInteger
                    | DataType::UnsignedInteger => {
                        Some(call("speedyspec_signed", text(take(inner))))
                    }
                    DataType::Decimal(info) => Some(cast(
                        call("speedyspec_decimal", text(take(inner))),
                        DataType::Numeric(info),
                    )),
                    DataType::Text | DataType::Char(_) | DataType::Varchar(_) => {
                        Some(text(take(inner)))
                    }
                    DataType::Date => Some(call("speedyspec_date", text(take(inner)))),
                    DataType::Datetime(_) | DataType::Timestamp(..) => {
                        Some(call("speedyspec_datetime", text(take(inner))))
                    }
                    DataType::Binary(None) => Some(template(
                        "convert_to(__a0, 'UTF8')",
                        vec![text(take(inner))],
                    )),
                    _ => None,
                };
                if let Some(r) = replacement {
                    *expr = r;
                }
            }
            Expr::BinaryOp {
                left,
                op: B::Eq | B::NotEq | B::Lt | B::LtEq | B::Gt | B::GtEq,
                right,
            } => {
                if self.is_text_column(left) && self.is_number(right) {
                    self.double(left);
                } else if self.is_text_column(right) && self.is_number(left) {
                    self.double(right);
                }
            }
            Expr::Between {
                expr: e, low, high, ..
            } => {
                if (self.is_number(low) || self.is_number(high)) && self.is_text_column(e) {
                    self.double(e);
                }
            }
            Expr::In {
                expr: e,
                target: InTarget::List(list),
                ..
            } if list.iter().any(|x| self.is_number(x)) && self.is_text_column(e) => self.double(e),
            _ => {}
        }
        ControlFlow::Continue(())
    }
}

impl RewritePass for PgExpressions {
    fn name(&self) -> &'static str {
        "pg_expressions"
    }

    fn rewrite(&self, stmt: &mut Statement, _cx: &PassContext) -> Result<Rewrite, DbError> {
        // The adapter's own statements (moving a sequence) are PostgreSQL already.
        if crate::plan::count_of(stmt.tree()) == crate::plan::RowCount::Hidden {
            return Ok(Rewrite::Continue);
        }
        let tables = tables_in(stmt.tree());
        let params = stmt.params().to_vec();
        let mut exprs = Exprs {
            schema: &self.schema,
            tables,
            params: &params,
            error: None,
        };
        let tree = stmt.tree_mut();
        match tree {
            Ast::Update(u) => u.selection = u.selection.take().map(truthy),
            Ast::Delete(d) => d.selection = d.selection.take().map(truthy),
            _ => {}
        }
        let _ = tree.visit(&mut exprs);
        match exprs.error {
            Some(e) => Err(e),
            None => Ok(Rewrite::Continue),
        }
    }
}
