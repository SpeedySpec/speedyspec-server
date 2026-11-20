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

    #[allow(dead_code)] // read by the DDL and write passes
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
