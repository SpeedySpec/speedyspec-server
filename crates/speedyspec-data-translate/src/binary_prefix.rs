//! `BinaryPrefixPass`: MySQL's `BINARY x` with MySQL's precedence. `sqlparser` 0.64 reads the prefix as a cast of
//! everything that follows it (`BINARY a = 'x' AND b` as `CAST((a = 'x' AND b) AS BINARY)`); MySQL binds it to the
//! one operand after it. The pass moves each such cast down onto its first operand and lifts what it swallowed back
//! above the operator that holds it.

use std::ops::ControlFlow;

use speedyspec_data::{DbError, Statement};
use speedyspec_data_query::{PassContext, Rewrite, RewritePass};
use sqlparser::ast::{BinaryOperator, CastKind, DataType, Expr, VisitMut, VisitorMut};

pub struct BinaryPrefixPass;

/// MySQL's binding strength of an operator node; `None` for an operand.
fn precedence(expr: &Expr) -> Option<u8> {
    use BinaryOperator as B;
    Some(match expr {
        Expr::BinaryOp { op, .. } => match op {
            B::Or => 1,
            B::Xor => 2,
            B::And => 3,
            B::Eq | B::NotEq | B::Lt | B::LtEq | B::Gt | B::GtEq | B::Spaceship => 5,
            B::BitwiseOr => 6,
            B::BitwiseAnd => 7,
            B::PGBitwiseShiftLeft | B::PGBitwiseShiftRight => 8,
            B::Plus | B::Minus => 9,
            B::Multiply | B::Divide | B::Modulo | B::MyIntegerDivide => 10,
            B::BitwiseXor => 11,
            _ => return None,
        },
        Expr::Like { .. } | Expr::ILike { .. } | Expr::RLike { .. } => 5,
        _ => return None,
    })
}

fn is_binary_cast(expr: &Expr) -> bool {
    matches!(
        expr,
        Expr::Cast {
            kind: CastKind::Cast,
            data_type: DataType::Binary(None),
            format: None,
            ..
        }
    )
}

fn binary_cast(expr: Expr) -> Expr {
    Expr::Cast {
        kind: CastKind::Cast,
        expr: Box::new(expr),
        data_type: DataType::Binary(None),
        format: None,
    }
}

/// The left-most operand of an operator chain.
fn first_operand(expr: &mut Expr) -> &mut Expr {
    if precedence(expr).is_none() {
        return expr;
    }
    match expr {
        Expr::BinaryOp { left, .. } => first_operand(left),
        Expr::Like { expr, .. } | Expr::ILike { expr, .. } | Expr::RLike { expr, .. } => {
            first_operand(expr)
        }
        _ => unreachable!("an operator node"),
    }
}

/// The right operand of an operator node.
fn right_operand(expr: &mut Expr) -> Option<&mut Expr> {
    match expr {
        Expr::BinaryOp { right, .. } => Some(right),
        Expr::Like { pattern, .. } | Expr::ILike { pattern, .. } | Expr::RLike { pattern, .. } => {
            Some(pattern)
        }
        _ => None,
    }
}

fn take(expr: &mut Expr) -> Expr {
    std::mem::replace(expr, Expr::value(sqlparser::ast::Value::Null))
}

/// `P(A, op(X, R))` with `op` weaker than `P` becomes `op(P(A, X), R)`, until no weaker operator sits on the right.
fn lift(expr: &mut Expr) {
    let Some(outer) = precedence(expr) else {
        return;
    };
    let Some(right) = right_operand(expr) else {
        return;
    };
    if !precedence(right).is_some_and(|inner| inner < outer) {
        return;
    }
    let mut lifted = take(right);
    let x = take(x_slot_of(&mut lifted));
    *right_operand(expr).expect("an operator node") = x;
    lift(expr);
    let p = take(expr);
    *x_slot_of(&mut lifted) = p;
    *expr = lifted;
    lift(expr);
}

struct Repair;

impl VisitorMut for Repair {
    type Break = ();

    fn post_visit_expr(&mut self, expr: &mut Expr) -> ControlFlow<()> {
        // CAST(op(L, R) AS BINARY) → op(CAST(L AS BINARY), R)
        if is_binary_cast(expr)
            && let Expr::Cast { expr: inner, .. } = expr
            && precedence(inner).is_some()
        {
            let mut chain = take(inner);
            let first = first_operand(&mut chain);
            let operand = take(first);
            *first = binary_cast(operand);
            *expr = chain;
        }
        lift(expr);
        ControlFlow::Continue(())
    }
}

fn x_slot_of(expr: &mut Expr) -> &mut Expr {
    match expr {
        Expr::BinaryOp { left, .. } => left,
        Expr::Like { expr, .. } | Expr::ILike { expr, .. } | Expr::RLike { expr, .. } => expr,
        _ => unreachable!("an operator node"),
    }
}

impl RewritePass for BinaryPrefixPass {
    fn name(&self) -> &'static str {
        "binary_prefix"
    }

    fn rewrite(&self, stmt: &mut Statement, _cx: &PassContext) -> Result<Rewrite, DbError> {
        let _ = stmt.tree_mut().visit(&mut Repair);
        Ok(Rewrite::Continue)
    }
}
