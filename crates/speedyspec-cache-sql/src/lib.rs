//! A Caching Store over any SQL Store (Adapter): one table of groups, keys, byte values and expiry times.
//!
//! Every statement is a template parsed once with `?` placeholders, or built with the query builder; nothing else
//! reaches the store. Times come from the worker's clock, never the engine's.

use std::rc::Rc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use sha2::{Digest, Sha256};
use speedyspec_cache::{CacheError, CachingStore};
use speedyspec_data::ast;
use speedyspec_data::{DbError, DbValue, Rows, SqlStore, Statement, UpsertForm};
use speedyspec_data_query::{Query, parse_one};

/// A group or key as the table stores it: over 191 characters, `sha256:` and the hex digest of its UTF-8 bytes.
pub fn stored_key(text: &str) -> String {
    if text.chars().count() <= 191 {
        return text.to_string();
    }
    let digest = Sha256::digest(text.as_bytes());
    format!(
        "sha256:{}",
        digest
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
    )
}

struct Templates {
    get: ast::Statement,
    insert: ast::Statement,
    delete_any: ast::Statement,
    delete_live: ast::Statement,
    delete_expired_key: ast::Statement,
    select_for_update: ast::Statement,
    update_value: ast::Statement,
    flush: ast::Statement,
    flush_group: ast::Statement,
    purge: ast::Statement,
    create: ast::Statement,
}

/// A Caching Store in one SQL table.
pub struct SqlCache {
    store: Rc<dyn SqlStore>,
    table: String,
    name: String,
    t: Templates,
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn expires_at(expiry: Option<Duration>) -> DbValue {
    match expiry {
        None => DbValue::Null,
        Some(d) => {
            let secs = d.as_secs() + u64::from(d.subsec_nanos() > 0);
            DbValue::Int(now().saturating_add(i64::try_from(secs).unwrap_or(i64::MAX)))
        }
    }
}

fn bytes(v: &DbValue) -> Vec<u8> {
    match v {
        DbValue::Text(t) => t.as_bytes().to_vec(),
        DbValue::Bytes(b) => b.clone(),
        other => other.to_mysql_text().unwrap_or_default(),
    }
}

fn int(v: &DbValue) -> Option<i64> {
    match v {
        DbValue::Int(i) => Some(*i),
        DbValue::UInt(u) => i64::try_from(*u).ok(),
        DbValue::Null => None,
        other => std::str::from_utf8(&bytes(other)).ok()?.parse().ok(),
    }
}

fn number(group: &str, key: &str, value: &[u8]) -> Result<i64, CacheError> {
    std::str::from_utf8(value)
        .ok()
        .and_then(|s| s.parse::<i64>().ok())
        .ok_or_else(|| CacheError::NotANumber {
            group: group.into(),
            key: key.into(),
        })
}

impl SqlCache {
    /// A cache in `table` (the full name, e.g. `wp_speedyspec_cache`) of `store`; the templates are parsed now.
    pub fn new(store: Rc<dyn SqlStore>, table: &str) -> Result<SqlCache, CacheError> {
        let name = format!("sql:{}", store.engine());
        let q = format!("`{}`", table.replace('`', "``"));
        let live = "(expires_at IS NULL OR expires_at > ?)";
        let parse = |sql: String| -> Result<ast::Statement, CacheError> {
            parse_one(&sql)
                .map(|s| s.into_parts().0)
                .map_err(|e| CacheError::Backend {
                    cache: name.clone(),
                    message: e.to_string(),
                })
        };
        let t = Templates {
            get: parse(format!(
                "SELECT cache_key, cache_value, expires_at FROM {q} WHERE cache_group = ? AND cache_key = ? AND {live}"
            ))?,
            insert: parse(format!(
                "INSERT INTO {q} (cache_group, cache_key, cache_value, expires_at) VALUES (?, ?, ?, ?)"
            ))?,
            delete_any: parse(format!(
                "DELETE FROM {q} WHERE cache_group = ? AND cache_key = ?"
            ))?,
            delete_live: parse(format!(
                "DELETE FROM {q} WHERE cache_group = ? AND cache_key = ? AND {live}"
            ))?,
            delete_expired_key: parse(format!(
                "DELETE FROM {q} WHERE cache_group = ? AND cache_key = ? AND expires_at IS NOT NULL AND expires_at <= ?"
            ))?,
            select_for_update: parse(format!(
                "SELECT cache_value FROM {q} WHERE cache_group = ? AND cache_key = ? AND {live} FOR UPDATE"
            ))?,
            update_value: parse(format!(
                "UPDATE {q} SET cache_value = ? WHERE cache_group = ? AND cache_key = ?"
            ))?,
            flush: parse(format!("DELETE FROM {q}"))?,
            flush_group: parse(format!("DELETE FROM {q} WHERE cache_group = ?"))?,
            purge: parse(format!(
                "DELETE FROM {q} WHERE expires_at IS NOT NULL AND expires_at <= ?"
            ))?,
            create: parse(format!(
                "CREATE TABLE IF NOT EXISTS {q} (cache_group varchar(191) NOT NULL, cache_key varchar(191) NOT NULL, \
                 cache_value longblob NOT NULL, expires_at bigint NULL, PRIMARY KEY (cache_group, cache_key), \
                 KEY expires_at (expires_at)) DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_bin"
            ))?,
        };
        Ok(SqlCache {
            store,
            table: table.to_string(),
            name,
            t,
        })
    }

    fn err(&self, e: DbError) -> CacheError {
        CacheError::Backend {
            cache: self.name.clone(),
            message: e.to_string(),
        }
    }

    fn stmt(tree: &ast::Statement, params: Vec<DbValue>) -> Statement {
        Statement::new(tree.clone(), params)
    }

    fn keyed(group: &str, key: &str) -> Vec<DbValue> {
        vec![
            DbValue::Text(stored_key(group)),
            DbValue::Text(stored_key(key)),
        ]
    }

    fn query(&self, tree: &ast::Statement, params: Vec<DbValue>) -> Result<Rows, CacheError> {
        self.store
            .query(&Self::stmt(tree, params))
            .map_err(|e| self.err(e))
    }

    fn execute(&self, tree: &ast::Statement, params: Vec<DbValue>) -> Result<u64, CacheError> {
        self.store
            .execute(&Self::stmt(tree, params))
            .map(|r| r.rows_affected)
            .map_err(|e| self.err(e))
    }

    /// Creates the table when it is absent.
    pub fn install(&self) -> Result<(), CacheError> {
        self.execute(&self.t.create, Vec::new()).map(|_| ())
    }

    /// Removes every expired entry; returns how many.
    pub fn purge_expired(&self) -> Result<u64, CacheError> {
        self.execute(&self.t.purge, vec![DbValue::Int(now())])
    }

    fn in_own_transaction<T>(
        &self,
        f: impl FnOnce() -> Result<T, CacheError>,
    ) -> Result<T, CacheError> {
        let own = self.store.capabilities().transactions && !self.store.in_transaction();
        if !own {
            return f();
        }
        self.store.begin().map_err(|e| self.err(e))?;
        match f() {
            Ok(v) => {
                self.store.commit().map_err(|e| self.err(e))?;
                Ok(v)
            }
            Err(e) => {
                let _ = self.store.rollback();
                Err(e)
            }
        }
    }

    fn insert_params(
        group: &str,
        key: &str,
        value: &[u8],
        expiry: Option<Duration>,
    ) -> Vec<DbValue> {
        let mut params = Self::keyed(group, key);
        params.push(DbValue::Bytes(value.to_vec()));
        params.push(expires_at(expiry));
        params
    }

    fn adjust(
        &self,
        group: &str,
        key: &str,
        f: impl Fn(i64) -> i64,
    ) -> Result<Option<i64>, CacheError> {
        self.in_own_transaction(|| {
            let mut params = Self::keyed(group, key);
            params.push(DbValue::Int(now()));
            let rows = self.query(&self.t.select_for_update, params)?;
            let Some(row) = rows.rows.first() else {
                return Ok(None);
            };
            let current = number(
                group,
                key,
                &bytes(row.get_index(0).unwrap_or(&DbValue::Null)),
            )?;
            let n = f(current);
            let mut params = vec![DbValue::Bytes(n.to_string().into_bytes())];
            params.extend(Self::keyed(group, key));
            self.execute(&self.t.update_value, params)?;
            Ok(Some(n))
        })
    }

    pub fn table(&self) -> &str {
        &self.table
    }
}

impl CachingStore for SqlCache {
    fn name(&self) -> &str {
        &self.name
    }

    fn get(&self, group: &str, key: &str) -> Result<Option<Vec<u8>>, CacheError> {
        let mut params = Self::keyed(group, key);
        params.push(DbValue::Int(now()));
        let rows = self.query(&self.t.get, params)?;
        Ok(rows
            .rows
            .first()
            .map(|r| bytes(r.get_index(1).unwrap_or(&DbValue::Null))))
    }

    fn get_many(&self, group: &str, keys: &[&str]) -> Result<Vec<Option<Vec<u8>>>, CacheError> {
        if keys.is_empty() {
            return Ok(Vec::new());
        }
        let stored: Vec<String> = keys.iter().map(|k| stored_key(k)).collect();
        let stmt = Query::table(&self.table)
            .select(&["cache_key", "cache_value", "expires_at"])
            .where_eq("cache_group", stored_key(group))
            .where_in("cache_key", stored.iter().map(|k| k.as_str()))
            .build()
            .map_err(|e| self.err(e))?;
        let rows = self.store.query(&stmt).map_err(|e| self.err(e))?;
        let now = now();
        let found: Vec<(Vec<u8>, Vec<u8>)> = rows
            .rows
            .iter()
            .filter(|r| r.get_index(2).and_then(int).is_none_or(|at| at > now))
            .map(|r| {
                (
                    bytes(r.get_index(0).unwrap_or(&DbValue::Null)),
                    bytes(r.get_index(1).unwrap_or(&DbValue::Null)),
                )
            })
            .collect();
        Ok(stored
            .iter()
            .map(|k| {
                found
                    .iter()
                    .find(|(fk, _)| fk == k.as_bytes())
                    .map(|(_, v)| v.clone())
            })
            .collect())
    }

    fn set(
        &self,
        group: &str,
        key: &str,
        value: &[u8],
        expiry: Option<Duration>,
    ) -> Result<(), CacheError> {
        let params = Self::insert_params(group, key, value, expiry);
        if self.store.capabilities().upsert != UpsertForm::None {
            let stmt = Query::insert_into(
                &self.table,
                &["cache_group", "cache_key", "cache_value", "expires_at"],
            )
            .values(params)
            .on_duplicate_update(&["cache_value", "expires_at"])
            .build()
            .map_err(|e| self.err(e))?;
            return self
                .store
                .execute(&stmt)
                .map(|_| ())
                .map_err(|e| self.err(e));
        }
        self.in_own_transaction(|| {
            self.execute(&self.t.delete_any, Self::keyed(group, key))?;
            self.execute(&self.t.insert, params.clone())?;
            Ok(())
        })
    }

    fn add(
        &self,
        group: &str,
        key: &str,
        value: &[u8],
        expiry: Option<Duration>,
    ) -> Result<bool, CacheError> {
        let mut params = Self::keyed(group, key);
        params.push(DbValue::Int(now()));
        self.execute(&self.t.delete_expired_key, params)?;
        match self.store.execute(&Self::stmt(
            &self.t.insert,
            Self::insert_params(group, key, value, expiry),
        )) {
            Ok(_) => Ok(true),
            Err(DbError::Constraint { .. }) => Ok(false),
            Err(e) => Err(self.err(e)),
        }
    }

    fn replace(
        &self,
        group: &str,
        key: &str,
        value: &[u8],
        expiry: Option<Duration>,
    ) -> Result<bool, CacheError> {
        if self.get(group, key)?.is_none() {
            return Ok(false);
        }
        self.set(group, key, value, expiry)?;
        Ok(true)
    }

    fn delete(&self, group: &str, key: &str) -> Result<bool, CacheError> {
        let mut params = Self::keyed(group, key);
        params.push(DbValue::Int(now()));
        let removed = self.execute(&self.t.delete_live, params)? > 0;
        // An expired row the delete left goes too.
        self.execute(&self.t.delete_any, Self::keyed(group, key))?;
        Ok(removed)
    }

    fn increment(&self, group: &str, key: &str, by: i64) -> Result<Option<i64>, CacheError> {
        self.adjust(group, key, |n| n.saturating_add(by))
    }

    fn decrement(&self, group: &str, key: &str, by: i64) -> Result<Option<i64>, CacheError> {
        self.adjust(group, key, |n| n.saturating_sub(by).max(0))
    }

    fn flush(&self) -> Result<(), CacheError> {
        self.execute(&self.t.flush, Vec::new()).map(|_| ())
    }

    fn flush_group(&self, group: &str) -> Result<(), CacheError> {
        self.execute(&self.t.flush_group, vec![DbValue::Text(stored_key(group))])
            .map(|_| ())
    }
}
