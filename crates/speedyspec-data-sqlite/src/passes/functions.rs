//! `sqlite_functions`: MySQL operators, casts, collations, locking clauses and statements SQLite spells otherwise.

use std::ops::ControlFlow;

use speedyspec_data::{DbError, DialectKind, Statement};
use speedyspec_data_query::{PassContext, Rewrite, RewritePass};
use speedyspec_data_translate::object_name;
use sqlparser::ast::{
    BinaryOperator, CastKind, DataType, DuplicateTreatment, Expr, Function, FunctionArg,
    FunctionArgExpr, FunctionArgumentClause, FunctionArguments, Ident, ObjectName, Query, Select,
    SelectItem, Statement as Ast, TableFactor, TransactionAccessMode, TransactionMode, Value,
    VisitMut, VisitorMut,
};
use sqlparser::dialect::SQLiteDialect;
use sqlparser::parser::Parser;

use super::column_types::CI_COLLATION;

pub struct SqliteFunctions;

/// The `information_schema` tables the engine fills in, MySQL-shaped.
const INFORMATION_SCHEMA: &[&str] = &["tables", "columns", "statistics"];

fn unsupported(construct: &str) -> DbError {
    DbError::Unsupported {
        construct: construct.into(),
        dialect: DialectKind::Sqlite,
    }
}

fn take(expr: &mut Expr) -> Expr {
    std::mem::replace(expr, Expr::value(Value::Null))
}

fn cast(expr: Expr, data_type: DataType) -> Expr {
    Expr::Cast {
        kind: CastKind::Cast,
        expr: Box::new(expr),
        data_type,
        format: None,
    }
}

fn collate(expr: Expr, name: &str) -> Expr {
    Expr::Collate {
        expr: Box::new(expr),
        collation: ObjectName::from(vec![Ident::new(name)]),
    }
}

fn function_name(f: &Function) -> String {
    object_name(&f.name).to_ascii_lowercase()
}

/// `CAST(x AS BLOB)`: SQLite's LIKE and REGEXP (the registered ones) compare a blob case-sensitively.
fn blob_operand(expr: &mut Expr) {
    if let Expr::Cast { data_type, .. } = expr
        && matches!(data_type, DataType::Binary(None))
    {
        *data_type = DataType::Blob(None);
    }
}

fn info_schema_table(name: &ObjectName) -> Option<String> {
    let parts: Vec<String> = name
        .0
        .iter()
        .map(|p| p.to_string().trim_matches(['`', '"']).to_string())
        .collect();
    match parts.as_slice() {
        [schema, table] if schema.eq_ignore_ascii_case("information_schema") => {
            Some(table.to_ascii_lowercase())
        }
        _ => None,
    }
}

struct Functions;

impl Functions {
    fn group_concat(f: &mut Function) -> Result<(), DbError> {
        let template = f.clone();
        let FunctionArguments::List(list) = &mut f.args else {
            return Ok(());
        };
        let mut separator = None;
        list.clauses.retain(|c| match c {
            FunctionArgumentClause::Separator(v) => {
                separator = Some(v.value.clone());
                false
            }
            _ => true,
        });
        let separator = separator.unwrap_or_else(|| Value::SingleQuotedString(",".into()));
        let distinct = matches!(list.duplicate_treatment, Some(DuplicateTreatment::Distinct));
        let comma = matches!(&separator, Value::SingleQuotedString(s) if s == ",");
        if distinct && !comma {
            return Err(unsupported("GROUP_CONCAT(DISTINCT ... SEPARATOR)"));
        }
        if list.args.len() > 1 {
            // MySQL joins several expressions per row before it separates the rows.
            let args = std::mem::take(&mut list.args);
            let mut concat = template;
            concat.name = ObjectName::from(vec![Ident::new("CONCAT")]);
            if let FunctionArguments::List(l) = &mut concat.args {
                l.args = args;
                l.clauses.clear();
                l.duplicate_treatment = None;
            }
            list.args = vec![FunctionArg::Unnamed(FunctionArgExpr::Expr(Expr::Function(
                concat,
            )))];
        }
        if !distinct {
            list.args
                .push(FunctionArg::Unnamed(FunctionArgExpr::Expr(Expr::value(
                    separator,
                ))));
        }
        f.name = ObjectName::from(vec![Ident::new("group_concat")]);
        Ok(())
    }
}

impl VisitorMut for Functions {
    type Break = DbError;

    fn pre_visit_query(&mut self, query: &mut Query) -> ControlFlow<DbError> {
        // BEGIN IMMEDIATE already holds the write lock.
        query.locks.clear();
        ControlFlow::Continue(())
    }

    fn pre_visit_select(&mut self, select: &mut Select) -> ControlFlow<DbError> {
        let reads_schema = select.from.iter().any(|t| {
            matches!(&t.relation, TableFactor::Table { name, .. } if info_schema_table(name).is_some())
        });
        for item in &mut select.projection {
            let SelectItem::UnnamedExpr(expr) = item else {
                continue;
            };
            let alias = match &*expr {
                Expr::Function(f) if function_name(f) == "last_insert_id" => {
                    Some("LAST_INSERT_ID()".to_string())
                }
                // MySQL 8 names information_schema columns in upper case.
                Expr::Identifier(c) if reads_schema => Some(c.value.to_ascii_uppercase()),
                _ => None,
            };
            if let Some(alias) = alias {
                *item = SelectItem::ExprWithAlias {
                    expr: take(expr),
                    alias: Ident::new(alias),
                };
            }
        }
        ControlFlow::Continue(())
    }

    fn pre_visit_table_factor(&mut self, factor: &mut TableFactor) -> ControlFlow<DbError> {
        if let TableFactor::Table { name, .. } = factor
            && let Some(table) = info_schema_table(name)
            && !INFORMATION_SCHEMA.contains(&table.as_str())
        {
            return ControlFlow::Break(unsupported(&format!("information_schema.{table}")));
        }
        ControlFlow::Continue(())
    }

    fn pre_visit_expr(&mut self, expr: &mut Expr) -> ControlFlow<DbError> {
        match expr {
            Expr::Like { expr, pattern, .. }
            | Expr::ILike { expr, pattern, .. }
            | Expr::RLike { expr, pattern, .. } => {
                blob_operand(expr);
                blob_operand(pattern);
            }
            Expr::MatchAgainst { .. } => {
                return ControlFlow::Break(unsupported("MATCH ... AGAINST"));
            }
            _ => {}
        }
        ControlFlow::Continue(())
    }

    fn post_visit_expr(&mut self, expr: &mut Expr) -> ControlFlow<DbError> {
        match expr {
            Expr::BinaryOp {
                left,
                op: op @ BinaryOperator::MyIntegerDivide,
                right,
            } => {
                **left = cast(take(left), DataType::Integer(None));
                **right = cast(take(right), DataType::Integer(None));
                *op = BinaryOperator::Divide;
            }
            Expr::BinaryOp {
                left,
                op: BinaryOperator::Divide,
                ..
            } => {
                // MySQL's `/` always divides as decimals; SQLite's divides integers as integers.
                **left = cast(take(left), DataType::Real);
            }
            Expr::Substring {
                special,
                substring_from,
                ..
            } if substring_from.is_some() => *special = true,
            Expr::RLike { regexp, .. } => *regexp = true,
            Expr::Collate { collation, .. } => {
                let name = object_name(collation).to_ascii_lowercase();
                let mapped = if name.ends_with("_bin") || name == "binary" {
                    Some("BINARY")
                } else if ["utf8", "latin1", "ascii", "ucs2"]
                    .iter()
                    .any(|p| name.starts_with(p))
                {
                    Some(CI_COLLATION)
                } else {
                    None
                };
                if let Some(mapped) = mapped {
                    *collation = ObjectName::from(vec![Ident::new(mapped)]);
                }
            }
            Expr::Cast { data_type, .. } => match data_type {
                DataType::Date => {
                    let Expr::Cast { expr: inner, .. } = take(expr) else {
                        unreachable!()
                    };
                    *expr = speedyspec_data_translate::function_call("date", vec![*inner]);
                }
                DataType::Datetime(_) | DataType::Time(..) | DataType::Timestamp(..) => {
                    *data_type = DataType::Text
                }
                DataType::Decimal(_) | DataType::Numeric(_) | DataType::Dec(_) => {
                    *data_type = DataType::Real
                }
                DataType::Char(_) | DataType::Character(_) | DataType::Varchar(_) => {
                    *data_type = DataType::Text
                }
                DataType::Binary(None) => {
                    let Expr::Cast { expr: inner, .. } = take(expr) else {
                        unreachable!()
                    };
                    *expr = collate(*inner, "BINARY");
                }
                _ => {}
            },
            Expr::Function(f) => match function_name(f).as_str() {
                "last_insert_id" => {
                    let no_args = match &f.args {
                        FunctionArguments::List(l) => l.args.is_empty(),
                        FunctionArguments::None => true,
                        _ => false,
                    };
                    if !no_args {
                        return ControlFlow::Break(unsupported("LAST_INSERT_ID(expr)"));
                    }
                    f.name = ObjectName::from(vec![Ident::new("last_insert_rowid")]);
                }
                "group_concat" => {
                    if let Err(e) = Functions::group_concat(f) {
                        return ControlFlow::Break(e);
                    }
                }
                _ => {}
            },
            _ => {}
        }
        ControlFlow::Continue(())
    }
}

fn sqlite_statement(sql: &str) -> Ast {
    Parser::parse_sql(&SQLiteDialect {}, sql)
        .unwrap_or_else(|e| panic!("{sql}: {e}"))
        .remove(0)
}

impl RewritePass for SqliteFunctions {
    fn name(&self) -> &'static str {
        "sqlite_functions"
    }

    fn rewrite(&self, stmt: &mut Statement, _cx: &PassContext) -> Result<Rewrite, DbError> {
        if let Ast::StartTransaction { modes, .. } = stmt.tree() {
            let read_only = modes.iter().any(|m| {
                matches!(
                    m,
                    TransactionMode::AccessMode(TransactionAccessMode::ReadOnly)
                )
            });
            *stmt.tree_mut() = sqlite_statement(if read_only {
                "BEGIN DEFERRED"
            } else {
                "BEGIN IMMEDIATE"
            });
            return Ok(Rewrite::Continue);
        }
        if let ControlFlow::Break(e) = stmt.tree_mut().visit(&mut Functions) {
            return Err(e);
        }
        Ok(Rewrite::Continue)
    }
}
