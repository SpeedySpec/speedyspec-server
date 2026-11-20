//! `PgZeroDates`: MySQL's zero date (`'0000-00-00 00:00:00'`, `'0000-00-00'`) as `-infinity`, which PostgreSQL stores
//! and sorts before every date as MySQL sorts its zero date, wherever it meets a date column.

use std::ops::ControlFlow;
use std::rc::Rc;

use speedyspec_data::{DbError, DbValue, Statement};
use speedyspec_data_query::{PassContext, Rewrite, RewritePass};
use sqlparser::ast::{
    AssignmentTarget, BinaryOperator, Expr, InTarget, SetExpr, Statement as Ast, Value, VisitMut,
    VisitorMut,
};

use crate::schema::{SchemaCache, tables_in};
use crate::types::is_temporal;

pub(crate) struct PgZeroDates {
    schema: Rc<SchemaCache>,
}

impl PgZeroDates {
    pub(crate) fn new(schema: Rc<SchemaCache>) -> PgZeroDates {
        PgZeroDates { schema }
    }
}

fn is_zero(text: &str) -> bool {
    text == "0000-00-00 00:00:00" || text == "0000-00-00"
}

struct Zero<'a> {
    schema: &'a SchemaCache,
    tables: Vec<(String, String)>,
    params: &'a mut Vec<DbValue>,
}

impl Zero<'_> {
    /// Whether the column holds latin1, whose values MySQL narrows to what latin1 can hold.
    fn is_latin1(&self, expr: &Expr) -> bool {
        let name = match expr {
            Expr::Identifier(i) => i.value.clone(),
            _ => return false,
        };
        self.tables.iter().any(|(_, t)| {
            self.schema.columns(t).is_some_and(|cols| {
                cols.iter().any(|c| {
                    c.name.eq_ignore_ascii_case(&name)
                        && c.collation
                            .as_deref()
                            .is_some_and(|k| k.starts_with("latin1"))
                })
            })
        })
    }

    fn narrow(&self, value: &mut Expr) {
        if matches!(value, Expr::Value(v) if matches!(v.value, Value::Null)) {
            return;
        }
        let inner = std::mem::replace(value, Expr::value(Value::Null));
        *value = super::functions::template("speedyspec_latin1(__a0)", vec![inner]);
    }

    fn is_date(&self, expr: &Expr) -> bool {
        self.schema
            .type_of(&self.tables, expr)
            .is_some_and(|t| is_temporal(&t))
    }

    /// The zero date in `value`, a literal or a param, as `-infinity`.
    fn fix(&mut self, value: &mut Expr) {
        let Expr::Value(v) = value else { return };
        match &mut v.value {
            Value::SingleQuotedString(s) | Value::DoubleQuotedString(s) if is_zero(s) => {
                *s = "-infinity".into()
            }
            Value::Placeholder(p) => {
                if let Some(n) = p.strip_prefix('?').and_then(|n| n.parse::<usize>().ok())
                    && let Some(DbValue::Text(s)) = self.params.get_mut(n.wrapping_sub(1))
                    && is_zero(s)
                {
                    *s = "-infinity".into();
                }
            }
            _ => {}
        }
    }
}

impl VisitorMut for Zero<'_> {
    type Break = ();

    fn post_visit_expr(&mut self, expr: &mut Expr) -> ControlFlow<()> {
        use BinaryOperator as B;
        match expr {
            Expr::BinaryOp {
                left,
                op: B::Eq | B::NotEq | B::Lt | B::LtEq | B::Gt | B::GtEq,
                right,
            } => {
                if self.is_date(left) {
                    self.fix(right);
                } else if self.is_date(right) {
                    self.fix(left);
                }
            }
            Expr::IsDistinctFrom(left, right) | Expr::IsNotDistinctFrom(left, right)
                if matches!((&**left, &**right), (Expr::Tuple(_), Expr::Tuple(_))) =>
            {
                if let (Expr::Tuple(columns), Expr::Tuple(values)) = (&**left, &mut **right) {
                    for (c, v) in columns.iter().zip(values.iter_mut()) {
                        if self.is_date(c) {
                            self.fix(v);
                        }
                    }
                }
            }
            Expr::IsDistinctFrom(left, right) | Expr::IsNotDistinctFrom(left, right) => {
                if self.is_date(left) {
                    self.fix(right);
                } else if self.is_date(right) {
                    self.fix(left);
                }
            }
            Expr::Between {
                expr: e, low, high, ..
            } if self.is_date(e) => {
                self.fix(low);
                self.fix(high);
            }
            Expr::In {
                expr: e,
                target: InTarget::List(list),
                ..
            } if self.is_date(e) => {
                for item in list.iter_mut() {
                    self.fix(item);
                }
            }
            _ => {}
        }
        ControlFlow::Continue(())
    }
}

impl RewritePass for PgZeroDates {
    fn name(&self) -> &'static str {
        "pg_zero_dates"
    }

    fn rewrite(&self, stmt: &mut Statement, _cx: &PassContext) -> Result<Rewrite, DbError> {
        let tables = tables_in(stmt.tree());
        let mut params = stmt.params().to_vec();
        let (tree, _) = stmt.clone().into_parts();
        let mut tree = tree;
        {
            let mut zero = Zero {
                schema: &self.schema,
                tables: tables.clone(),
                params: &mut params,
            };
            match &mut tree {
                Ast::Update(update) => {
                    for a in &mut update.assignments {
                        if let AssignmentTarget::ColumnName(name) = &a.target {
                            let column = Expr::CompoundIdentifier(
                                name.0
                                    .iter()
                                    .filter_map(|p| p.as_ident().cloned())
                                    .collect(),
                            );
                            let column = match &column {
                                Expr::CompoundIdentifier(parts) if parts.len() == 1 => {
                                    Expr::Identifier(parts[0].clone())
                                }
                                _ => column,
                            };
                            if zero.is_date(&column) {
                                zero.fix(&mut a.value);
                            }
                            if zero.is_latin1(&column) {
                                zero.narrow(&mut a.value);
                            }
                        }
                    }
                }
                Ast::Insert(insert) => {
                    let latin1: Vec<bool> = insert
                        .columns
                        .iter()
                        .map(|c| {
                            let ident = c.0.last().and_then(|p| p.as_ident().cloned());
                            ident.is_some_and(|i| zero.is_latin1(&Expr::Identifier(i)))
                        })
                        .collect();
                    let dates: Vec<bool> = insert
                        .columns
                        .iter()
                        .map(|c| {
                            let ident = c.0.last().and_then(|p| p.as_ident().cloned());
                            ident.is_some_and(|i| zero.is_date(&Expr::Identifier(i)))
                        })
                        .collect();
                    if let Some(source) = &mut insert.source
                        && let SetExpr::Values(values) = source.body.as_mut()
                    {
                        for row in &mut values.rows {
                            for ((value, date), narrow) in
                                row.content.iter_mut().zip(&dates).zip(&latin1)
                            {
                                if *date {
                                    zero.fix(value);
                                }
                                if *narrow {
                                    zero.narrow(value);
                                }
                            }
                        }
                    }
                }
                _ => {}
            }
            let _ = tree.visit(&mut zero);
        }
        *stmt = Statement::new(tree, params);
        Ok(Rewrite::Continue)
    }
}
