//! `RedbStore`: one table per keyspace; each value an envelope of 8 bytes little-endian `expires_at` (Unix ms,
//! `0` = never) then the value.

use std::ops::Bound;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use redb::{
    Database, DatabaseError, ReadableDatabase, ReadableTable, TableDefinition, TableError,
    TableHandle,
};
use speedyspec_data::{Capabilities, DbError, KeyValues, NoSqlStore, UpsertForm};
use speedyspec_data_counter::checked_increment;

type Def<'a> = TableDefinition<'a, &'static [u8], &'static [u8]>;

pub struct RedbStore {
    engine: String,
    db: Arc<Database>,
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// When a TTL ends; under a millisecond lives a millisecond, as on Valkey.
fn expires_at(ttl: Option<Duration>) -> u64 {
    // A far TTL saturates to the end of time rather than overflowing.
    ttl.map(|t| now_ms().saturating_add(u64::try_from(t.as_millis()).unwrap_or(u64::MAX).max(1)))
        .unwrap_or(0)
}

fn envelope(value: &[u8], expires: u64) -> Vec<u8> {
    let mut out = Vec::with_capacity(8 + value.len());
    out.extend(expires.to_le_bytes());
    out.extend(value);
    out
}

/// The envelope's expiry and value; a short envelope reads as a value that never expires.
fn open(bytes: &[u8]) -> (u64, &[u8]) {
    match bytes.split_first_chunk::<8>() {
        Some((head, value)) => (u64::from_le_bytes(*head), value),
        None => (0, bytes),
    }
}

fn live(expires: u64, now: u64) -> bool {
    expires == 0 || now < expires
}

impl RedbStore {
    /// Opens (creating) the file; another process holding it is a `Connection` error.
    pub fn open(engine: &str, file: &Path) -> Result<RedbStore, DbError> {
        let db = Database::create(file).map_err(|e| match e {
            DatabaseError::DatabaseAlreadyOpen => DbError::Connection {
                engine: engine.to_string(),
                message: format!(
                    "{} is open in another process; is a second owner running?",
                    file.display()
                ),
            },
            other => DbError::Connection {
                engine: engine.to_string(),
                message: format!("{}: {other}", file.display()),
            },
        })?;
        Ok(RedbStore {
            engine: engine.to_string(),
            db: Arc::new(db),
        })
    }

    fn storage(&self, e: impl Into<redb::Error>) -> DbError {
        DbError::Engine {
            engine: self.engine.clone(),
            code: Some("storage".into()),
            message: e.into().to_string(),
        }
    }

    /// Runs `f` on the keyspace's table in a read transaction; a missing table gives `missing`.
    fn read<T>(
        &self,
        keyspace: &str,
        missing: T,
        f: impl FnOnce(&redb::ReadOnlyTable<&'static [u8], &'static [u8]>) -> Result<T, redb::Error>,
    ) -> Result<T, DbError> {
        let txn = self.db.begin_read().map_err(|e| self.storage(e))?;
        match txn.open_table(Def::new(keyspace)) {
            Ok(table) => f(&table).map_err(|e| self.storage(e)),
            Err(TableError::TableDoesNotExist(_)) => Ok(missing),
            Err(e) => Err(self.storage(e)),
        }
    }

    /// Runs `f` on the keyspace's table in one write transaction, committed when `f` succeeds.
    fn write<T>(
        &self,
        keyspace: &str,
        f: impl FnOnce(&mut redb::Table<&'static [u8], &'static [u8]>) -> Result<T, DbError>,
    ) -> Result<T, DbError> {
        let txn = self.db.begin_write().map_err(|e| self.storage(e))?;
        let out = {
            let mut table = txn
                .open_table(Def::new(keyspace))
                .map_err(|e| self.storage(e))?;
            f(&mut table)?
        };
        txn.commit().map_err(|e| self.storage(e))?;
        Ok(out)
    }

    /// The live value and its expiry.
    fn current(
        &self,
        table: &impl ReadableTable<&'static [u8], &'static [u8]>,
        key: &[u8],
    ) -> Result<Option<(u64, Vec<u8>)>, DbError> {
        let guard = table.get(key).map_err(|e| self.storage(e))?;
        Ok(guard.and_then(|g| {
            let (expires, value) = open(g.value());
            live(expires, now_ms()).then(|| (expires, value.to_vec()))
        }))
    }

    /// Removes up to `batch` expired entries across every keyspace; how many it removed.
    pub fn sweep_expired(&self, batch: usize) -> Result<usize, DbError> {
        let txn = self.db.begin_write().map_err(|e| self.storage(e))?;
        let names: Vec<String> = txn
            .list_tables()
            .map_err(|e| self.storage(e))?
            .map(|t| t.name().to_string())
            .collect();
        let now = now_ms();
        let mut removed = 0;
        for name in names {
            if removed >= batch {
                break;
            }
            let mut table = txn
                .open_table(Def::new(&name))
                .map_err(|e| self.storage(e))?;
            let mut expired = Vec::new();
            for entry in table.iter().map_err(|e| self.storage(e))? {
                let (k, v) = entry.map_err(|e| self.storage(e))?;
                if !live(open(v.value()).0, now) {
                    expired.push(k.value().to_vec());
                    if removed + expired.len() >= batch {
                        break;
                    }
                }
            }
            for k in &expired {
                table.remove(k.as_slice()).map_err(|e| self.storage(e))?;
            }
            removed += expired.len();
        }
        txn.commit().map_err(|e| self.storage(e))?;
        Ok(removed)
    }
}

/// What ReDB offers, through the store or the client alike.
pub(crate) fn capabilities() -> Capabilities {
    Capabilities {
        transactions: false,
        savepoints: false,
        ordered_ranges: true,
        expiry: true,
        atomic_increment: true,
        returning: false,
        upsert: UpsertForm::None,
    }
}

impl NoSqlStore for RedbStore {
    fn engine(&self) -> &str {
        &self.engine
    }

    fn capabilities(&self) -> Capabilities {
        capabilities()
    }

    fn get(&self, keyspace: &str, key: &[u8]) -> Result<Option<Vec<u8>>, DbError> {
        Ok(self.get_many(keyspace, &[key])?.pop().flatten())
    }

    fn get_many(&self, keyspace: &str, keys: &[&[u8]]) -> Result<Vec<Option<Vec<u8>>>, DbError> {
        let now = now_ms();
        self.read(keyspace, vec![None; keys.len()], |table| {
            keys.iter()
                .map(|k| {
                    Ok(table.get(*k)?.and_then(|g| {
                        let (expires, value) = open(g.value());
                        live(expires, now).then(|| value.to_vec())
                    }))
                })
                .collect()
        })
    }

    fn put(
        &self,
        keyspace: &str,
        key: &[u8],
        value: &[u8],
        ttl: Option<Duration>,
    ) -> Result<(), DbError> {
        self.write(keyspace, |table| {
            match ttl {
                // A zero TTL deletes, as Valkey's SET with an expiry already past does.
                Some(t) if t.is_zero() => {
                    table.remove(key).map_err(|e| self.storage(e))?;
                }
                _ => {
                    table
                        .insert(key, envelope(value, expires_at(ttl)).as_slice())
                        .map_err(|e| self.storage(e))?;
                }
            }
            Ok(())
        })
    }

    fn put_if_absent(
        &self,
        keyspace: &str,
        key: &[u8],
        value: &[u8],
        ttl: Option<Duration>,
    ) -> Result<bool, DbError> {
        self.write(keyspace, |table| {
            if self.current(table, key)?.is_some() || matches!(ttl, Some(t) if t.is_zero()) {
                return Ok(false);
            }
            table
                .insert(key, envelope(value, expires_at(ttl)).as_slice())
                .map_err(|e| self.storage(e))?;
            Ok(true)
        })
    }

    fn delete(&self, keyspace: &str, key: &[u8]) -> Result<bool, DbError> {
        self.write(keyspace, |table| {
            let found = self.current(table, key)?.is_some();
            table.remove(key).map_err(|e| self.storage(e))?;
            Ok(found)
        })
    }

    fn range(
        &self,
        keyspace: &str,
        start: Bound<&[u8]>,
        end: Bound<&[u8]>,
        limit: Option<usize>,
    ) -> Result<KeyValues, DbError> {
        // An empty or inverted range is empty, where redb would refuse it.
        let inverted = match (start, end) {
            (Bound::Included(s), Bound::Included(e)) => s > e,
            (Bound::Included(s) | Bound::Excluded(s), Bound::Included(e) | Bound::Excluded(e)) => {
                s >= e
            }
            _ => false,
        };
        if inverted {
            return Ok(Vec::new());
        }
        let now = now_ms();
        self.read(keyspace, Vec::new(), |table| {
            let mut out = Vec::new();
            for entry in table.range::<&[u8]>((start, end))? {
                if out.len() >= limit.unwrap_or(usize::MAX) {
                    break;
                }
                let (k, v) = entry?;
                let (expires, value) = open(v.value());
                if live(expires, now) {
                    out.push((k.value().to_vec(), value.to_vec()));
                }
            }
            Ok(out)
        })
    }

    fn increment(&self, keyspace: &str, key: &[u8], by: i64) -> Result<i64, DbError> {
        self.write(keyspace, |table| {
            let current = self.current(table, key)?;
            let (next, bytes) = checked_increment(
                &self.engine,
                current.as_ref().map(|(_, v)| v.as_slice()),
                by,
            )?;
            // The increment keeps the key's expiry.
            let expires = current.map(|(at, _)| at).unwrap_or(0);
            table
                .insert(key, envelope(&bytes, expires).as_slice())
                .map_err(|e| self.storage(e))?;
            Ok(next)
        })
    }

    fn clear(&self, keyspace: &str) -> Result<(), DbError> {
        let txn = self.db.begin_write().map_err(|e| self.storage(e))?;
        txn.delete_table(Def::new(keyspace))
            .map_err(|e| self.storage(e))?;
        txn.commit().map_err(|e| self.storage(e))
    }
}
