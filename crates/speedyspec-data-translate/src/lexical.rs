//! `LexicalPass`: MySQL's spellings of identifiers, strings, limits and operators in their portable forms.

use std::ops::ControlFlow;

use speedyspec_data::{DbError, Statement};
use speedyspec_data_query::{PassContext, Rewrite, RewritePass};
use sqlparser::ast::{
    BinaryOperator, Expr, Ident, LimitClause, Offset, OffsetRows, Query, Value, ValueWithSpan,
    VisitMut, VisitorMut,
};

/// Backticks to double quotes, `"`-strings to single quotes, `LIMIT o, c` to `LIMIT c OFFSET o`, `<=>` to
/// `IS NOT DISTINCT FROM`, `||` (MySQL's `OR`) to `OR`, and MySQL's default `ESCAPE '\'` on every `LIKE`.
pub struct LexicalPass;

/// A literal row count no signed 64-bit `LIMIT` holds.
fn past_i64(expr: &Expr) -> bool {
    matches!(expr, Expr::Value(ValueWithSpan { value: Value::Number(n, _), .. })
        if n.parse::<u128>().is_ok_and(|n| n > i64::MAX as u128))
}

struct Lexical;

impl VisitorMut for Lexical {
    type Break = ();

    fn pre_visit_ident(&mut self, ident: &mut Ident) -> ControlFlow<()> {
        if ident.quote_style == Some('`') {
            ident.quote_style = Some('"');
        }
        ControlFlow::Continue(())
    }

    fn pre_visit_value(&mut self, value: &mut ValueWithSpan) -> ControlFlow<()> {
        if let Value::DoubleQuotedString(s) = &value.value {
            value.value = Value::SingleQuotedString(s.clone());
        }
        ControlFlow::Continue(())
    }

    fn pre_visit_query(&mut self, query: &mut Query) -> ControlFlow<()> {
        if let Some(LimitClause::OffsetCommaLimit { offset, limit }) = &query.limit_clause {
            query.limit_clause = Some(LimitClause::LimitOffset {
                limit: Some(limit.clone()),
                offset: Some(Offset {
                    value: offset.clone(),
                    rows: OffsetRows::None,
                }),
                limit_by: Vec::new(),
            });
        }
        // MySQL's "every row after an offset" is a row count of 2^64 - 1, past every other engine's 64-bit limit:
        // it means no limit. A dialect that cannot write OFFSET alone spells the missing limit its own way.
        if let Some(LimitClause::LimitOffset { limit, offset, .. }) = &mut query.limit_clause
            && limit.as_ref().is_some_and(past_i64)
        {
            *limit = None;
            if offset.is_none() {
                query.limit_clause = None;
            }
        }
        ControlFlow::Continue(())
    }

    fn post_visit_expr(&mut self, expr: &mut Expr) -> ControlFlow<()> {
        match expr {
            Expr::BinaryOp {
                left,
                op: BinaryOperator::Spaceship,
                right,
            } => {
                *expr = Expr::IsNotDistinctFrom(left.clone(), right.clone());
            }
            Expr::BinaryOp {
                op: op @ BinaryOperator::StringConcat,
                ..
            } => *op = BinaryOperator::Or,
            Expr::Like {
                escape_char: escape @ None,
                ..
            } => {
                *escape = Some(Box::new(Expr::Value(
                    Value::SingleQuotedString("\\".into()).with_empty_span(),
                )));
            }
            _ => {}
        }
        ControlFlow::Continue(())
    }
}

impl RewritePass for LexicalPass {
    fn name(&self) -> &'static str {
        "lexical"
    }

    fn rewrite(&self, stmt: &mut Statement, _cx: &PassContext) -> Result<Rewrite, DbError> {
        let _ = stmt.tree_mut().visit(&mut Lexical);
        Ok(Rewrite::Continue)
    }
}
