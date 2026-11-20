//! `ExpressionPass`: MySQL's casts, interval arithmetic and system variables as calls the engines provide.

use std::ops::ControlFlow;

use speedyspec_data::{DbError, DialectKind, Statement};
use speedyspec_data_query::{PassContext, Rewrite, RewritePass};
use sqlparser::ast::{
    BinaryOperator, CastKind, DataType, Expr, Function, FunctionArg, FunctionArgExpr,
    FunctionArgumentList, FunctionArguments, Ident, Interval, ObjectName, Select, SelectItem,
    Value, VisitMut, VisitorMut,
};

/// `CAST(x AS SIGNED|UNSIGNED)` to `BIGINT`, `AS CHAR[(n)]` to `TEXT`; `DATE_ADD`, `ADDDATE`, `DATE_SUB`, `SUBDATE`
/// and `d ± INTERVAL n U` to `speedyspec_date_add|sub(d, n, 'U')`; `@@[SESSION.|GLOBAL.|LOCAL.]name` to
/// `speedyspec_sysvar('name')`, aliased with its own text when it is an unaliased projection.
pub struct ExpressionPass;

/// A call `name(args…)`, for passes building portable expressions.
pub fn function_call(name: &str, args: Vec<Expr>) -> Expr {
    Expr::Function(Function {
        name: ObjectName::from(vec![Ident::new(name)]),
        uses_odbc_syntax: false,
        parameters: FunctionArguments::None,
        args: FunctionArguments::List(FunctionArgumentList {
            duplicate_treatment: None,
            args: args
                .into_iter()
                .map(|e| FunctionArg::Unnamed(FunctionArgExpr::Expr(e)))
                .collect(),
            clauses: Vec::new(),
        }),
        filter: None,
        null_treatment: None,
        over: None,
        within_group: Vec::new(),
    })
}

fn text(s: &str) -> Expr {
    Expr::Value(Value::SingleQuotedString(s.into()).with_empty_span())
}

/// The system variable a `@@…` expression names, and its text.
fn sysvar(expr: &Expr) -> Option<(String, String)> {
    match expr {
        Expr::Identifier(ident) => {
            let name = ident.value.strip_prefix("@@")?;
            Some((name.to_string(), ident.value.clone()))
        }
        Expr::CompoundIdentifier(parts) if parts.len() == 2 => {
            let scope = parts[0].value.strip_prefix("@@")?;
            if !["SESSION", "GLOBAL", "LOCAL"]
                .iter()
                .any(|s| s.eq_ignore_ascii_case(scope))
            {
                return None;
            }
            Some((
                parts[1].value.clone(),
                format!("{}.{}", parts[0].value, parts[1].value),
            ))
        }
        _ => None,
    }
}

/// The amount and unit of `INTERVAL n U`.
fn interval(interval: &Interval) -> Option<(Expr, String)> {
    let unit = interval.leading_field.as_ref()?.to_string();
    (interval.last_field.is_none()).then(|| ((*interval.value).clone(), unit))
}

fn function_args(f: &Function) -> Option<Vec<Expr>> {
    let FunctionArguments::List(list) = &f.args else {
        return None;
    };
    list.args
        .iter()
        .map(|a| match a {
            FunctionArg::Unnamed(FunctionArgExpr::Expr(e)) => Some(e.clone()),
            _ => None,
        })
        .collect()
}

struct Expressions {
    target: DialectKind,
    error: Option<DbError>,
}

impl Expressions {
    fn unsupported(&mut self, construct: String) {
        self.error.get_or_insert(DbError::Unsupported {
            construct,
            dialect: self.target,
        });
    }

    fn date(&mut self, expr: &mut Expr) {
        let (name, args) = match expr {
            Expr::Function(f) => {
                let name = f.name.to_string().to_ascii_uppercase();
                let add = match name.as_str() {
                    "DATE_ADD" | "ADDDATE" => true,
                    "DATE_SUB" | "SUBDATE" => false,
                    _ => return,
                };
                let Some(args) = function_args(f) else { return };
                let [date, amount] = args.as_slice() else {
                    return;
                };
                let (value, unit) = match amount {
                    Expr::Interval(i) => match interval(i) {
                        Some(found) => found,
                        None => return self.unsupported(format!("{expr}")),
                    },
                    // `ADDDATE(d, n)`: n days.
                    other => (other.clone(), "DAY".to_string()),
                };
                (
                    if add {
                        "speedyspec_date_add"
                    } else {
                        "speedyspec_date_sub"
                    },
                    vec![date.clone(), value, text(&unit)],
                )
            }
            Expr::BinaryOp { left, op, right } => {
                let add = match op {
                    BinaryOperator::Plus => true,
                    BinaryOperator::Minus => false,
                    _ => return,
                };
                let (date, i) = match (&**left, &**right) {
                    (d, Expr::Interval(i)) => (d.clone(), i),
                    (Expr::Interval(i), d) if add => (d.clone(), i),
                    _ => return,
                };
                let Some((value, unit)) = interval(i) else {
                    return self.unsupported(format!("{expr}"));
                };
                (
                    if add {
                        "speedyspec_date_add"
                    } else {
                        "speedyspec_date_sub"
                    },
                    vec![date, value, text(&unit)],
                )
            }
            _ => return,
        };
        *expr = function_call(name, args);
    }
}

impl VisitorMut for Expressions {
    type Break = ();

    fn pre_visit_select(&mut self, select: &mut Select) -> ControlFlow<()> {
        for item in &mut select.projection {
            if let SelectItem::UnnamedExpr(e) = item
                && let Some((_, original)) = sysvar(e)
            {
                *item = SelectItem::ExprWithAlias {
                    expr: e.clone(),
                    alias: Ident::with_quote('"', original),
                };
            }
        }
        ControlFlow::Continue(())
    }

    fn post_visit_expr(&mut self, expr: &mut Expr) -> ControlFlow<()> {
        if let Expr::Cast {
            kind: CastKind::Cast,
            data_type,
            ..
        } = expr
        {
            match data_type {
                DataType::Signed
                | DataType::SignedInteger
                | DataType::Unsigned
                | DataType::UnsignedInteger => {
                    *data_type = DataType::BigInt(None);
                }
                DataType::Char(_) => *data_type = DataType::Text,
                _ => {}
            }
        }
        if let Some((name, _)) = sysvar(expr) {
            *expr = function_call("speedyspec_sysvar", vec![text(&name)]);
            return ControlFlow::Continue(());
        }
        self.date(expr);
        ControlFlow::Continue(())
    }
}

impl RewritePass for ExpressionPass {
    fn name(&self) -> &'static str {
        "expressions"
    }

    fn rewrite(&self, stmt: &mut Statement, cx: &PassContext) -> Result<Rewrite, DbError> {
        let mut v = Expressions {
            target: cx.target,
            error: None,
        };
        let _ = stmt.tree_mut().visit(&mut v);
        match v.error {
            Some(e) => Err(e),
            None => Ok(Rewrite::Continue),
        }
    }
}
