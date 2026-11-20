//! Store contracts: SQL, NoSQL and Direct.

use std::any::Any;
use std::ops::Bound;
use std::time::Duration;

use crate::{Capabilities, DbError, Dialect, ExecResult, Rows, Statement};

/// One column as MySQL's `SHOW FULL COLUMNS` describes it, types written MySQL's way.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnDef {
    pub name: String,
    pub sql_type: String,
    pub nullable: bool,
    pub key: String,
    pub default: Option<String>,
    pub extra: String,
    pub collation: Option<String>,
}

/// One index and its columns, each with an optional prefix length.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexDef {
    pub name: String,
    pub unique: bool,
    pub columns: Vec<(String, Option<u32>)>,
}

/// Reads a SQL engine's schema, answering in MySQL's terms whatever the engine.
pub trait Introspector {
    fn server_version(&self) -> Result<String, DbError>;
    fn tables(&self, like: Option<&str>) -> Result<Vec<String>, DbError>;
    fn columns(&self, table: &str) -> Result<Vec<ColumnDef>, DbError>;
    fn indexes(&self, table: &str) -> Result<Vec<IndexDef>, DbError>;
    fn table_collation(&self, table: &str) -> Result<Option<String>, DbError>;

    /// MySQL's `Index_type` for an index (`BTREE`, `FULLTEXT`, `SPATIAL`); `None` when the engine does not say.
    fn index_type(&self, table: &str, index: &str) -> Result<Option<String>, DbError> {
        let _ = (table, index);
        Ok(None)
    }
}

/// A relational store. Statements only; a caller with SQL text parses it first.
///
/// `in_transaction` also reflects transaction-control statements run through `execute`; `reset` rolls back an open
/// transaction and restores the connection's session state.
pub trait SqlStore {
    fn engine(&self) -> &str;
    fn dialect(&self) -> &dyn Dialect;
    fn capabilities(&self) -> Capabilities;
    fn execute(&self, stmt: &Statement) -> Result<ExecResult, DbError>;
    fn query(&self, stmt: &Statement) -> Result<Rows, DbError>;
    fn begin(&self) -> Result<(), DbError>;
    fn commit(&self) -> Result<(), DbError>;
    fn rollback(&self) -> Result<(), DbError>;
    fn savepoint(&self, name: &str) -> Result<(), DbError>;
    fn release_savepoint(&self, name: &str) -> Result<(), DbError>;
    fn rollback_to_savepoint(&self, name: &str) -> Result<(), DbError>;
    fn in_transaction(&self) -> bool;
    fn introspect(&self) -> &dyn Introspector;
    fn reset(&self) -> Result<(), DbError>;
}

/// Key-value pairs returned by `NoSqlStore::range`, in key order.
pub type KeyValues = Vec<(Vec<u8>, Vec<u8>)>;

/// A key-value store of byte keys and values in named keyspaces.
pub trait NoSqlStore {
    fn engine(&self) -> &str;
    fn capabilities(&self) -> Capabilities;
    fn get(&self, keyspace: &str, key: &[u8]) -> Result<Option<Vec<u8>>, DbError>;
    fn get_many(&self, keyspace: &str, keys: &[&[u8]]) -> Result<Vec<Option<Vec<u8>>>, DbError>;
    fn put(
        &self,
        keyspace: &str,
        key: &[u8],
        value: &[u8],
        ttl: Option<Duration>,
    ) -> Result<(), DbError>;
    /// Atomic: true when written, false when the key already existed.
    fn put_if_absent(
        &self,
        keyspace: &str,
        key: &[u8],
        value: &[u8],
        ttl: Option<Duration>,
    ) -> Result<bool, DbError>;
    fn delete(&self, keyspace: &str, key: &[u8]) -> Result<bool, DbError>;
    fn range(
        &self,
        keyspace: &str,
        start: Bound<&[u8]>,
        end: Bound<&[u8]>,
        limit: Option<usize>,
    ) -> Result<KeyValues, DbError>;
    fn increment(&self, keyspace: &str, key: &[u8], by: i64) -> Result<i64, DbError>;
    fn clear(&self, keyspace: &str) -> Result<(), DbError>;
}

/// A store's health as it reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Health {
    pub ok: bool,
    pub detail: String,
}

/// Any other storage system, reached through its own client; per-system repositories downcast with `as_any`.
pub trait DirectStore {
    fn engine(&self) -> &str;
    fn kind(&self) -> &'static str;
    fn connect(&self) -> Result<(), DbError>;
    fn health(&self) -> Result<Health, DbError>;
    fn close(&self) -> Result<(), DbError>;
    fn as_any(&self) -> &dyn Any;
}
