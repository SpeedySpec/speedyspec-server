//! `PgIdentifiers`: every table, column, alias and CTE name double-quoted. A column is written as the table stores it
//! (MySQL matches column names in any case, PostgreSQL's quoted names match exactly), and a projected column written
//! in another case keeps the name it was written with, as MySQL names it.

use std::ops::ControlFlow;
use std::rc::Rc;

use speedyspec_data::{DbError, Statement};
use speedyspec_data_query::{PassContext, Rewrite, RewritePass};
use speedyspec_data_translate::object_name;
use sqlparser::ast::{
    AssignmentTarget, Expr, Ident, ObjectName, ObjectNamePart, Select, SelectItem,
    Statement as Ast, TableFactor, TableObject, Visit, VisitMut, Visitor, VisitorMut,
};

use crate::schema::SchemaCache;

/// Names PostgreSQL gives meaning to, which quoting would turn into ordinary identifiers.
const UNQUOTED: &[&str] = &["excluded", "ctid", "xmax"];

pub(crate) struct PgIdentifiers {
    schema: Rc<SchemaCache>,
}

impl PgIdentifiers {
    pub(crate) fn new(schema: Rc<SchemaCache>) -> PgIdentifiers {
        PgIdentifiers { schema }
    }
}

/// The statement's tables: (label it is referred to by, table name).
struct Scope(Vec<(String, String)>, Vec<String>);

impl Visitor for Scope {
    type Break = ();

    fn pre_visit_table_factor(&mut self, factor: &TableFactor) -> ControlFlow<()> {
        if let TableFactor::Table { name, alias, .. } = factor {
            let table = object_name(name);
            let label = alias
                .as_ref()
                .map_or_else(|| table.clone(), |a| a.name.value.clone());
            self.0.push((label, table));
        }
        ControlFlow::Continue(())
    }

    fn pre_visit_select(&mut self, select: &Select) -> ControlFlow<()> {
        for item in &select.projection {
            if let SelectItem::ExprWithAlias { alias, .. } = item {
                self.1.push(alias.value.clone());
            }
        }
        ControlFlow::Continue(())
    }
}

fn ident_mut(part: &mut ObjectNamePart) -> Option<&mut Ident> {
    match part {
        ObjectNamePart::Identifier(i) => Some(i),
        _ => None,
    }
}

struct Resolve<'a> {
    schema: &'a SchemaCache,
    tables: Vec<(String, String)>,
    aliases: Vec<String>,
}

impl Resolve<'_> {
    /// The stored spelling of an unqualified column: the first table in scope that has it, else a select alias.
    fn column(&self, written: &str) -> Option<String> {
        self.tables
            .iter()
            .find_map(|(_, table)| self.schema.resolve(table, written))
            .or_else(|| {
                self.aliases
                    .iter()
                    .find(|a| a.eq_ignore_ascii_case(written))
                    .cloned()
            })
    }

    /// A qualified column: the label as the FROM wrote it, and the stored column spelling.
    fn qualified(&self, label: &str, written: &str) -> (Option<String>, Option<String>) {
        match self
            .tables
            .iter()
            .find(|(l, _)| l.eq_ignore_ascii_case(label))
        {
            Some((l, table)) => (Some(l.clone()), self.schema.resolve(table, written)),
            None => (None, None),
        }
    }

    /// The expression with resolved spellings, and the name MySQL would give it when that differs.
    fn rename(&self, expr: &mut Expr) -> Option<String> {
        match expr {
            Expr::Identifier(i) if !UNQUOTED.iter().any(|u| u.eq_ignore_ascii_case(&i.value)) => {
                let written = i.value.clone();
                if let Some(stored) = self.column(&written) {
                    i.value = stored;
                }
                (i.value != written).then_some(written)
            }
            Expr::CompoundIdentifier(parts) if parts.len() >= 2 => {
                let n = parts.len();
                let written = parts[n - 1].value.clone();
                let (label, stored) = self.qualified(&parts[n - 2].value, &written);
                if let Some(label) = label {
                    parts[n - 2].value = label;
                }
                if let Some(stored) = stored {
                    parts[n - 1].value = stored;
                }
                (parts[n - 1].value != written).then_some(written)
            }
            _ => None,
        }
    }
}

impl VisitorMut for Resolve<'_> {
    type Break = ();

    fn pre_visit_select(&mut self, select: &mut Select) -> ControlFlow<()> {
        for item in &mut select.projection {
            if let SelectItem::UnnamedExpr(expr) = item
                && let Some(written) = self.rename(expr)
            {
                let expr = std::mem::replace(expr, Expr::value(sqlparser::ast::Value::Null));
                *item = SelectItem::ExprWithAlias {
                    expr,
                    alias: Ident::new(written),
                };
            }
        }
        ControlFlow::Continue(())
    }

    fn post_visit_expr(&mut self, expr: &mut Expr) -> ControlFlow<()> {
        self.rename(expr);
        ControlFlow::Continue(())
    }
}

struct Quote;

impl VisitorMut for Quote {
    type Break = ();

    fn pre_visit_ident(&mut self, ident: &mut Ident) -> ControlFlow<()> {
        if !UNQUOTED
            .iter()
            .any(|u| u.eq_ignore_ascii_case(&ident.value))
        {
            ident.quote_style = Some('"');
        }
        ControlFlow::Continue(())
    }

    fn post_visit_expr(&mut self, expr: &mut Expr) -> ControlFlow<()> {
        let bare = |name: &mut ObjectName| {
            name.0
                .iter_mut()
                .filter_map(ident_mut)
                .for_each(|i| i.quote_style = None)
        };
        match expr {
            Expr::Function(f) => bare(&mut f.name),
            Expr::Collate { collation, .. } => {
                // Collation names are case-sensitive identifiers in PostgreSQL; the passes write them as needed.
                let _ = collation;
            }
            _ => {}
        }
        ControlFlow::Continue(())
    }
}

/// The target's columns written as the table stores them (INSERT column lists, UPDATE SET targets).
fn resolve_targets(stmt: &mut Ast, schema: &SchemaCache, tables: &[(String, String)]) {
    let resolve_in = |table: &str, name: &mut ObjectName| {
        if let Some(i) = name.0.last_mut().and_then(ident_mut)
            && let Some(stored) = schema.resolve(table, &i.value)
        {
            i.value = stored;
        }
    };
    match stmt {
        Ast::Insert(insert) => {
            if let TableObject::TableName(name) = &insert.table {
                let table = object_name(name);
                for column in &mut insert.columns {
                    resolve_in(&table, column);
                }
            }
        }
        Ast::Update(update) => {
            for assignment in &mut update.assignments {
                if let AssignmentTarget::ColumnName(name) = &mut assignment.target {
                    let table = match name.0.len() {
                        1 => tables.first().map(|(_, t)| t.clone()),
                        _ => {
                            let label = name.0[name.0.len() - 2].to_string();
                            tables
                                .iter()
                                .find(|(l, _)| l.eq_ignore_ascii_case(&label))
                                .map(|(_, t)| t.clone())
                        }
                    };
                    if let Some(table) = table {
                        resolve_in(&table, name);
                    }
                }
            }
        }
        _ => {}
    }
}

/// A type PgDdl writes out whole (`text COLLATE "x"`, which sqlparser cannot hold) stays as written.
fn unquote_written_types(stmt: &mut Ast) {
    if let Ast::AlterTable(alter) = stmt {
        for op in &mut alter.operations {
            if let sqlparser::ast::AlterTableOperation::AlterColumn {
                op:
                    sqlparser::ast::AlterColumnOperation::SetDataType {
                        data_type: sqlparser::ast::DataType::Custom(name, _),
                        ..
                    },
                ..
            } = op
            {
                name.0
                    .iter_mut()
                    .filter_map(ident_mut)
                    .for_each(|i| i.quote_style = None);
            }
        }
    }
}

impl RewritePass for PgIdentifiers {
    fn name(&self) -> &'static str {
        "pg_identifiers"
    }

    fn rewrite(&self, stmt: &mut Statement, _cx: &PassContext) -> Result<Rewrite, DbError> {
        let mut scope = Scope(Vec::new(), Vec::new());
        let _ = stmt.tree().visit(&mut scope);
        if let Ast::Insert(insert) = stmt.tree()
            && let TableObject::TableName(name) = &insert.table
        {
            let table = object_name(name);
            scope.0.push((table.clone(), table));
        }
        let ddl = stmt.kind() == speedyspec_data::StatementKind::Ddl;
        if !ddl {
            resolve_targets(stmt.tree_mut(), &self.schema, &scope.0);
            let mut resolve = Resolve {
                schema: &self.schema,
                tables: scope.0,
                aliases: scope.1,
            };
            let _ = stmt.tree_mut().visit(&mut resolve);
        }
        let _ = stmt.tree_mut().visit(&mut Quote);
        unquote_written_types(stmt.tree_mut());
        Ok(Rewrite::Continue)
    }
}
