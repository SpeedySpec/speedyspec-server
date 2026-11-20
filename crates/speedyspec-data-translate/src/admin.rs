//! `AdminPass`: MySQL's administrative statements, which every engine answers in MySQL's shape (`EmulatedOp`).

use std::ops::ControlFlow;

use speedyspec_data::{DbError, DialectKind, EmulatedOp, Statement};
use speedyspec_data_query::{PassContext, Rewrite, RewritePass};
use sqlparser::ast::{
    Expr, FunctionArguments, SelectItem, SetExpr, ShowCreateObject, ShowStatementFilter,
    ShowStatementFilterPosition, ShowStatementOptions, Statement as Ast, Visit, Visitor,
};

use crate::names::{literal_text, object_name};

/// `SHOW …`, `DESCRIBE`, `SET NAMES`, `SET [SESSION] name = literal`, `SELECT FOUND_ROWS()`, and the statements MySQL
/// runs for their side effects only (`LOCK`/`UNLOCK TABLES`, `ANALYZE TABLE`), as emulations.
pub struct AdminPass;

fn unsupported(construct: &str, target: DialectKind) -> DbError {
    DbError::Unsupported {
        construct: construct.into(),
        dialect: target,
    }
}

fn like(options: &ShowStatementOptions) -> Option<String> {
    match &options.filter_position {
        Some(ShowStatementFilterPosition::Infix(ShowStatementFilter::Like(p)))
        | Some(ShowStatementFilterPosition::Suffix(ShowStatementFilter::Like(p))) => {
            Some(p.clone())
        }
        _ => None,
    }
}

fn table_in(options: &ShowStatementOptions) -> Option<String> {
    options
        .show_in
        .as_ref()?
        .parent_name
        .as_ref()
        .map(object_name)
}

fn is_found_rows(expr: &Expr) -> bool {
    matches!(expr, Expr::Function(f) if f.name.to_string().eq_ignore_ascii_case("FOUND_ROWS")
    && match &f.args {
        FunctionArguments::None => true,
        FunctionArguments::List(list) => list.args.is_empty(),
        FunctionArguments::Subquery(_) => false,
    })
}

struct FindFoundRows(bool);

impl Visitor for FindFoundRows {
    type Break = ();

    fn pre_visit_expr(&mut self, expr: &Expr) -> ControlFlow<()> {
        if is_found_rows(expr) {
            self.0 = true;
            return ControlFlow::Break(());
        }
        ControlFlow::Continue(())
    }
}

/// `SELECT FOUND_ROWS()` alone (aliased or not), with nothing else in the statement.
fn bare_found_rows(tree: &Ast) -> bool {
    let Ast::Query(query) = tree else {
        return false;
    };
    let SetExpr::Select(select) = &*query.body else {
        return false;
    };
    select.from.is_empty()
        && select.selection.is_none()
        && query.limit_clause.is_none()
        && select.projection.len() == 1
        && match &select.projection[0] {
            SelectItem::UnnamedExpr(e) | SelectItem::ExprWithAlias { expr: e, .. } => {
                is_found_rows(e)
            }
            _ => false,
        }
}

fn emulate(tree: &Ast, target: DialectKind) -> Result<Option<EmulatedOp>, DbError> {
    use sqlparser::ast::Set;
    Ok(Some(match tree {
        Ast::ShowTables { show_options, .. } => EmulatedOp::ShowTables {
            like: like(show_options),
        },
        Ast::ShowColumns {
            full, show_options, ..
        } => {
            let table = table_in(show_options)
                .ok_or_else(|| unsupported("SHOW COLUMNS without a table", target))?;
            EmulatedOp::ShowColumns {
                table,
                full: *full,
                like: like(show_options),
            }
        }
        Ast::ExplainTable { table_name, .. } => EmulatedOp::ShowColumns {
            table: object_name(table_name),
            full: false,
            like: None,
        },
        Ast::ShowCreate {
            obj_type: ShowCreateObject::Table,
            obj_name,
        } => EmulatedOp::ShowCreateTable {
            table: object_name(obj_name),
        },
        Ast::ShowVariables { filter, .. } => EmulatedOp::ShowVariables {
            like: match filter {
                Some(ShowStatementFilter::Like(p)) => Some(p.clone()),
                None => None,
                Some(ShowStatementFilter::Where(condition)) => Some(
                    variable_name_pattern(condition)
                        .ok_or_else(|| unsupported("SHOW VARIABLES WHERE", target))?,
                ),
                Some(_) => return Err(unsupported("SHOW VARIABLES WHERE", target)),
            },
        },
        // sqlparser 0.64 reads these as a list of words.
        Ast::ShowVariable { variable } => {
            let words: Vec<String> = variable
                .iter()
                .map(|i| i.value.to_ascii_uppercase())
                .collect();
            match words
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>()
                .as_slice()
            {
                ["INDEX" | "INDEXES" | "KEYS", "FROM" | "IN", ..] if variable.len() > 2 => {
                    EmulatedOp::ShowIndex {
                        table: variable[2..]
                            .iter()
                            .map(|i| i.value.clone())
                            .collect::<Vec<_>>()
                            .join("."),
                    }
                }
                ["TABLE", "STATUS"] => EmulatedOp::ShowTableStatus { like: None },
                ["TABLE", "STATUS", ..] => {
                    return Err(unsupported(
                        "SHOW TABLE STATUS LIKE (the parser drops the pattern)",
                        target,
                    ));
                }
                _ => return Ok(None),
            }
        }
        Ast::Set(Set::SetNames {
            charset_name,
            collation_name,
        }) => EmulatedOp::SetSession {
            name: "names".into(),
            value: match collation_name {
                Some(collation) => format!("{} {collation}", charset_name.value),
                None => charset_name.value.clone(),
            },
        },
        Ast::Set(Set::SingleAssignment {
            variable, values, ..
        }) => {
            let parts: Vec<&str> = variable
                .0
                .iter()
                .filter_map(|p| p.as_ident())
                .map(|i| i.value.as_str())
                .collect();
            let name = match parts.as_slice() {
                [scope, name]
                    if ["@@SESSION", "@@LOCAL"]
                        .iter()
                        .any(|s| s.eq_ignore_ascii_case(scope)) =>
                {
                    name.to_string()
                }
                [scope, _] if scope.eq_ignore_ascii_case("@@GLOBAL") => {
                    return Err(unsupported("SET GLOBAL", target));
                }
                [name] if name.starts_with("@@") => name.trim_start_matches("@@").to_string(),
                [name] if name.starts_with('@') => {
                    return Err(unsupported("user variable", target));
                }
                [name] => name.to_string(),
                _ => return Err(unsupported("SET of a qualified name", target)),
            }
            .to_ascii_lowercase();
            let value = values
                .iter()
                .map(literal_text)
                .collect::<Vec<_>>()
                .join(", ");
            // `autocommit` is a SetSession like any variable; the engine opens or ends the transaction it implies.
            EmulatedOp::SetSession { name, value }
        }
        Ast::Set(_) => return Err(unsupported("this form of SET", target)),
        Ast::LockTables { .. } | Ast::UnlockTables | Ast::Analyze(_) => EmulatedOp::Noop,
        // The engine has one database: it accepts its own name and refuses others, as MySQL refuses an unknown one.
        Ast::Use(sqlparser::ast::Use::Object(name) | sqlparser::ast::Use::Database(name)) => {
            EmulatedOp::SetSession {
                name: "database".into(),
                value: object_name(name),
            }
        }
        query if bare_found_rows(query) => EmulatedOp::FoundRows,
        _ => return Ok(None),
    }))
}

/// `Variable_name = 'x'` as the `LIKE` pattern matching exactly `x`, and `Variable_name LIKE 'p'` as `p`.
fn variable_name_pattern(condition: &sqlparser::ast::Expr) -> Option<String> {
    use sqlparser::ast::{BinaryOperator, Expr};
    let is_name = |e: &Expr| matches!(e, Expr::Identifier(i) if i.value.eq_ignore_ascii_case("variable_name"));
    match condition {
        Expr::BinaryOp {
            left,
            op: BinaryOperator::Eq,
            right,
        } if is_name(left) => {
            let exact = literal_text(right);
            Some(
                exact
                    .replace('\\', "\\\\")
                    .replace('%', "\\%")
                    .replace('_', "\\_"),
            )
        }
        Expr::Like {
            negated: false,
            expr,
            pattern,
            escape_char: None,
            ..
        } if is_name(expr) => Some(literal_text(pattern)),
        _ => None,
    }
}

impl RewritePass for AdminPass {
    fn name(&self) -> &'static str {
        "admin"
    }

    fn rewrite(&self, stmt: &mut Statement, cx: &PassContext) -> Result<Rewrite, DbError> {
        if let Some(op) = emulate(stmt.tree(), cx.target)? {
            return Ok(Rewrite::Emulate(op));
        }
        let mut found = FindFoundRows(false);
        let _ = stmt.tree().visit(&mut found);
        if found.0 {
            return Err(unsupported("FOUND_ROWS() in an expression", cx.target));
        }
        Ok(Rewrite::Continue)
    }
}
