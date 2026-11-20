//! Engines (Abstract Factory): one configured connection target and the stores it opens.

use std::collections::BTreeMap;
use std::rc::Rc;

use crate::{DbError, DirectStore, NoSqlStore, SqlStore};

/// The technology behind an engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EngineKind {
    MySql,
    Postgres,
    Sqlite,
    Redb,
    Valkey,
    File,
    Memory,
}

/// An opened engine, by store kind.
#[derive(Clone)]
pub enum EngineHandle {
    Sql(Rc<dyn SqlStore>),
    NoSql(Rc<dyn NoSqlStore>),
    Direct(Rc<dyn DirectStore>),
}

impl EngineHandle {
    pub fn as_sql(&self) -> Option<&Rc<dyn SqlStore>> {
        match self {
            EngineHandle::Sql(store) => Some(store),
            _ => None,
        }
    }

    pub fn as_nosql(&self) -> Option<&Rc<dyn NoSqlStore>> {
        match self {
            EngineHandle::NoSql(store) => Some(store),
            _ => None,
        }
    }

    pub fn as_direct(&self) -> Option<&Rc<dyn DirectStore>> {
        match self {
            EngineHandle::Direct(store) => Some(store),
            _ => None,
        }
    }
}

/// One engine's configuration: its name, its URL (the scheme picks the factory) and options.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineConfig {
    pub name: String,
    pub url: String,
    pub options: BTreeMap<String, String>,
}

/// A configured engine; `open` connects and gives its store.
pub trait Engine {
    fn name(&self) -> &str;
    fn kind(&self) -> EngineKind;
    fn open(&self) -> Result<EngineHandle, DbError>;
}

/// Builds engines for one URL scheme.
pub trait EngineFactory {
    fn scheme(&self) -> &'static str;
    fn create(&self, config: EngineConfig) -> Result<Rc<dyn Engine>, DbError>;
}

/// Finds opened engines by name; a unit of work commits in `commit_order`.
pub trait EngineLookup {
    fn handle(&self, engine: &str) -> Result<EngineHandle, DbError>;
    fn commit_order(&self) -> Vec<String>;
}
