//! What the passes know of the schema: each table's columns and indexes, read once from the introspector.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use speedyspec_data::{ColumnDef, IndexDef, Introspector};

type Columns = Option<Rc<[ColumnDef]>>;
type Indexes = Option<Rc<[IndexDef]>>;

pub(crate) struct SchemaCache {
    introspector: Option<Rc<dyn Introspector>>,
    columns: RefCell<HashMap<String, Columns>>,
    indexes: RefCell<HashMap<String, Indexes>>,
}

impl SchemaCache {
    pub(crate) fn new(introspector: Option<Rc<dyn Introspector>>) -> SchemaCache {
        SchemaCache {
            introspector,
            columns: RefCell::new(HashMap::new()),
            indexes: RefCell::new(HashMap::new()),
        }
    }

    pub(crate) fn introspector(&self) -> Option<&dyn Introspector> {
        self.introspector.as_deref()
    }

    /// The table's columns; `None` without an introspector or for a table it does not know.
    pub(crate) fn columns(&self, table: &str) -> Columns {
        if let Some(hit) = self.columns.borrow().get(table) {
            return hit.clone();
        }
        let found: Columns = self
            .introspector
            .as_ref()
            .and_then(|i| i.columns(table).ok())
            .filter(|c| !c.is_empty())
            .map(Rc::from);
        self.columns
            .borrow_mut()
            .insert(table.to_string(), found.clone());
        found
    }

    pub(crate) fn indexes(&self, table: &str) -> Indexes {
        if let Some(hit) = self.indexes.borrow().get(table) {
            return hit.clone();
        }
        let found: Indexes = self
            .introspector
            .as_ref()
            .and_then(|i| i.indexes(table).ok())
            .map(Rc::from);
        self.indexes
            .borrow_mut()
            .insert(table.to_string(), found.clone());
        found
    }

    /// The stored spelling of the column `written` names, matched as MySQL matches column names (any case).
    pub(crate) fn resolve(&self, table: &str, written: &str) -> Option<String> {
        self.columns(table)?
            .iter()
            .find(|c| c.name.eq_ignore_ascii_case(written))
            .map(|c| c.name.clone())
    }

    pub(crate) fn clear(&self) {
        self.columns.borrow_mut().clear();
        self.indexes.borrow_mut().clear();
    }
}

/// The statement's tables, each with the label it is referred to by: (label, table name).
pub(crate) fn tables_in(tree: &sqlparser::ast::Statement) -> Vec<(String, String)> {
    use speedyspec_data_translate::object_name;
    use sqlparser::ast::{Statement as Ast, TableFactor, TableObject, Visit, Visitor};
    struct Tables(Vec<(String, String)>);
    impl Visitor for Tables {
        type Break = ();
        fn pre_visit_table_factor(&mut self, factor: &TableFactor) -> std::ops::ControlFlow<()> {
            if let TableFactor::Table { name, alias, .. } = factor {
                let table = object_name(name);
                let label = alias
                    .as_ref()
                    .map_or_else(|| table.clone(), |a| a.name.value.clone());
                self.0.push((label, table));
            }
            std::ops::ControlFlow::Continue(())
        }
    }
    let mut tables = Tables(Vec::new());
    let _ = tree.visit(&mut tables);
    if let Ast::Insert(insert) = tree
        && let TableObject::TableName(name) = &insert.table
    {
        let table = object_name(name);
        tables.0.push((table.clone(), table));
    }
    tables.0
}

impl SchemaCache {
    /// The MySQL type of the column `expr` names, within `tables`.
    pub(crate) fn type_of(
        &self,
        tables: &[(String, String)],
        expr: &sqlparser::ast::Expr,
    ) -> Option<speedyspec_data_translate::MySqlType> {
        use sqlparser::ast::Expr;
        let find = |table: &str, column: &str| {
            self.columns(table)?
                .iter()
                .find(|c| c.name.eq_ignore_ascii_case(column))
                .and_then(|c| speedyspec_data_translate::MySqlType::parse(&c.sql_type).ok())
        };
        match expr {
            Expr::Identifier(c) => tables.iter().find_map(|(_, t)| find(t, &c.value)),
            Expr::CompoundIdentifier(parts) if parts.len() >= 2 => {
                let label = &parts[parts.len() - 2].value;
                let (_, table) = tables.iter().find(|(l, _)| l.eq_ignore_ascii_case(label))?;
                find(table, &parts[parts.len() - 1].value)
            }
            Expr::Nested(inner) => self.type_of(tables, inner),
            _ => None,
        }
    }
}
