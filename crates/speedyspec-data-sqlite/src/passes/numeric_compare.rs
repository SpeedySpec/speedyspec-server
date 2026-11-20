//! `sqlite_numeric_compare`: MySQL compares a string column with a number as numbers; SQLite would compare text, so
//! such a column is cast to `REAL` first.

use std::collections::HashMap;
use std::ops::ControlFlow;

use speedyspec_data::{DbError, DbValue, Introspector, Statement};
use speedyspec_data_query::{PassContext, Rewrite, RewritePass};
use speedyspec_data_translate::{TypeFamily, family_of, object_name};
use sqlparser::ast::{
    BinaryOperator, CastKind, DataType, Expr, InTarget, TableFactor, UnaryOperator, Value, Visit,
    VisitMut, Visitor, VisitorMut,
};

pub struct SqliteNumericCompare;

/// Each table the statement reads, by alias and by name.
struct Tables(Vec<(String, String)>);

impl Visitor for Tables {
    type Break = ();

    fn pre_visit_table_factor(&mut self, factor: &TableFactor) -> ControlFlow<()> {
        if let TableFactor::Table { name, alias, .. } = factor {
            let table = object_name(name);
            if let Some(alias) = alias {
                self.0.push((alias.name.value.clone(), table.clone()));
            }
            self.0.push((table.clone(), table));
        }
        ControlFlow::Continue(())
    }
}

struct Compare<'a> {
    tables: Vec<(String, String)>,
    columns: HashMap<String, Vec<(String, Option<TypeFamily>)>>,
    introspector: &'a dyn Introspector,
    params: &'a [DbValue],
}

impl Compare<'_> {
    fn family(&mut self, table: &str, column: &str) -> Option<TypeFamily> {
        if !self.columns.contains_key(table) {
            let columns = self
                .introspector
                .columns(table)
                .unwrap_or_default()
                .into_iter()
                .map(|c| (c.name, family_of(&c.sql_type)))
                .collect();
            self.columns.insert(table.to_string(), columns);
        }
        self.columns[table]
            .iter()
            .find(|(c, _)| c.eq_ignore_ascii_case(column))
            .and_then(|(_, f)| *f)
    }

    /// Whether `expr` names a column MySQL stores as a string.
    fn is_text_column(&mut self, expr: &Expr) -> bool {
        let family = match expr {
            Expr::Identifier(c) => {
                let tables: Vec<String> = self.tables.iter().map(|(_, t)| t.clone()).collect();
                tables.iter().find_map(|t| self.family(t, &c.value))
            }
            Expr::CompoundIdentifier(parts) if parts.len() == 2 => {
                let table = self
                    .tables
                    .iter()
                    .find(|(label, _)| label.eq_ignore_ascii_case(&parts[0].value))
                    .map(|(_, t)| t.clone());
                table.and_then(|t| self.family(&t, &parts[1].value))
            }
            _ => None,
        };
        matches!(family, Some(TypeFamily::Text | TypeFamily::Enum))
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
                op: UnaryOperator::Minus | UnaryOperator::Plus,
                expr,
            } => self.is_number(expr),
            _ => false,
        }
    }
}

fn real(expr: &mut Expr) {
    let inner = std::mem::replace(expr, Expr::value(Value::Null));
    *expr = Expr::Cast {
        kind: CastKind::Cast,
        expr: Box::new(inner),
        data_type: DataType::Real,
        format: None,
    };
}

impl VisitorMut for Compare<'_> {
    type Break = ();

    fn post_visit_expr(&mut self, expr: &mut Expr) -> ControlFlow<()> {
        use BinaryOperator as B;
        match expr {
            Expr::BinaryOp {
                left,
                op: B::Eq | B::NotEq | B::Lt | B::LtEq | B::Gt | B::GtEq,
                right,
            } => {
                if self.is_text_column(left) && self.is_number(right) {
                    real(left);
                } else if self.is_text_column(right) && self.is_number(left) {
                    real(right);
                }
            }
            Expr::Between {
                expr, low, high, ..
            } => {
                if (self.is_number(low) || self.is_number(high)) && self.is_text_column(expr) {
                    real(expr);
                }
            }
            Expr::In {
                expr,
                target: InTarget::List(list),
                ..
            } if list.iter().any(|e| self.is_number(e)) && self.is_text_column(expr) => real(expr),
            _ => {}
        }
        ControlFlow::Continue(())
    }
}

impl RewritePass for SqliteNumericCompare {
    fn name(&self) -> &'static str {
        "sqlite_numeric_compare"
    }

    fn rewrite(&self, stmt: &mut Statement, cx: &PassContext) -> Result<Rewrite, DbError> {
        let Some(introspector) = cx.introspector else {
            return Ok(Rewrite::Continue);
        };
        let mut tables = Tables(Vec::new());
        let _ = stmt.tree().visit(&mut tables);
        if tables.0.is_empty() {
            return Ok(Rewrite::Continue);
        }
        let params = stmt.params().to_vec();
        let mut compare = Compare {
            tables: tables.0,
            columns: HashMap::new(),
            introspector,
            params: &params,
        };
        let _ = stmt.tree_mut().visit(&mut compare);
        Ok(Rewrite::Continue)
    }
}
