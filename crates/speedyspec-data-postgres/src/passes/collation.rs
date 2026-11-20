//! `PgCollation`: MySQL's `LIKE` and `REGEXP` under a case-insensitive collation, which PostgreSQL refuses on a
//! nondeterministic collation, as `ILIKE` and `~*` on the operand recollated to ICU's root; MySQL's binary collations
//! as byte order.

use std::ops::ControlFlow;

use speedyspec_data::{DbError, Statement};
use speedyspec_data_query::{PassContext, Rewrite, RewritePass};
use speedyspec_data_translate::object_name;
use sqlparser::ast::{BinaryOperator, Expr, Ident, ObjectName, Value, VisitMut, VisitorMut};

pub(crate) struct PgCollation {
    pub(crate) schema: std::rc::Rc<crate::schema::SchemaCache>,
}

fn take(expr: &mut Expr) -> Expr {
    std::mem::replace(expr, Expr::value(Value::Null))
}

fn is_byte_order(expr: &Expr) -> bool {
    matches!(expr, Expr::Collate { collation, .. } if object_name(collation) == "C")
}

fn icu(expr: Expr) -> Expr {
    Expr::Collate {
        expr: Box::new(expr),
        collation: ObjectName::from(vec![Ident::new("und-x-icu")]),
    }
}

struct Collate<'a> {
    schema: &'a crate::schema::SchemaCache,
    tables: Vec<(String, String)>,
}

impl Collate<'_> {
    /// MySQL's LIKE reads a number as its text; PostgreSQL's wants text.
    fn as_text(&self, expr: Expr) -> Expr {
        let text = match &expr {
            Expr::Value(_) | Expr::Collate { .. } => true,
            Expr::Identifier(_) | Expr::CompoundIdentifier(_) => self
                .schema
                .type_of(&self.tables, &expr)
                .is_none_or(|t| crate::types::is_text(&t)),
            Expr::Function(_)
            | Expr::Nested(_)
            | Expr::Cast {
                data_type: sqlparser::ast::DataType::Text,
                ..
            } => {
                !matches!(&expr, Expr::Function(f) if speedyspec_data_translate::object_name(&f.name).starts_with("speedyspec_"))
                    && !matches!(&expr, Expr::Nested(_))
            }
            _ => false,
        };
        if text {
            expr
        } else {
            super::expressions::cast(expr, sqlparser::ast::DataType::Text)
        }
    }
}

impl VisitorMut for Collate<'_> {
    type Break = ();

    fn post_visit_expr(&mut self, expr: &mut Expr) -> ControlFlow<()> {
        match expr {
            Expr::Collate { collation, .. } => {
                let name = object_name(collation).to_ascii_lowercase();
                if name.ends_with("_bin") || name == "binary" {
                    *collation = ObjectName::from(vec![Ident::new("C")]);
                } else if ["utf8", "latin1", "ascii", "ucs2"]
                    .iter()
                    .any(|p| name.starts_with(p))
                {
                    *collation =
                        ObjectName::from(vec![Ident::new(crate::passes::ddl::CI_COLLATION)]);
                }
            }
            Expr::Like {
                negated,
                any,
                expr: e,
                pattern,
                escape_char,
            } => {
                if !(is_byte_order(e) || is_byte_order(pattern)) {
                    *expr = Expr::ILike {
                        negated: *negated,
                        any: *any,
                        expr: Box::new(icu(self.as_text(take(e)))),
                        pattern: Box::new(take(pattern)),
                        escape_char: escape_char.take(),
                    };
                }
            }
            Expr::RLike {
                negated,
                expr: e,
                pattern,
                ..
            } => {
                let sensitive = is_byte_order(e) || is_byte_order(pattern);
                let op = match (sensitive, *negated) {
                    (true, false) => BinaryOperator::PGRegexMatch,
                    (true, true) => BinaryOperator::PGRegexNotMatch,
                    (false, false) => BinaryOperator::PGRegexIMatch,
                    (false, true) => BinaryOperator::PGRegexNotIMatch,
                };
                let left = if sensitive {
                    take(e)
                } else {
                    icu(self.as_text(take(e)))
                };
                *expr = Expr::BinaryOp {
                    left: Box::new(left),
                    op,
                    right: Box::new(take(pattern)),
                };
            }
            _ => {}
        }
        ControlFlow::Continue(())
    }
}

impl RewritePass for PgCollation {
    fn name(&self) -> &'static str {
        "pg_collation"
    }

    fn rewrite(&self, stmt: &mut Statement, _cx: &PassContext) -> Result<Rewrite, DbError> {
        let tables = crate::schema::tables_in(stmt.tree());
        let _ = stmt.tree_mut().visit(&mut Collate {
            schema: &self.schema,
            tables,
        });
        Ok(Rewrite::Continue)
    }
}
