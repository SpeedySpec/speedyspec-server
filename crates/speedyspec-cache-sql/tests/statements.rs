use std::cell::{Cell, RefCell};
use std::rc::Rc;

use speedyspec_cache::CachingStore;
use speedyspec_cache_sql::{SqlCache, stored_key};
use speedyspec_data::*;

struct NoDialect;
impl Dialect for NoDialect {
    fn kind(&self) -> DialectKind {
        DialectKind::MySql
    }
    fn prepare(&self, stmt: &Statement) -> Result<Prepared, DbError> {
        Ok(Prepared::Sql(vec![EmittedSql {
            sql: stmt.tree().to_string(),
            params: stmt.params().to_vec(),
        }]))
    }
}
struct NoIntrospector;
impl Introspector for NoIntrospector {
    fn server_version(&self) -> Result<String, DbError> {
        Ok("8.4".into())
    }
    fn tables(&self, _: Option<&str>) -> Result<Vec<String>, DbError> {
        Ok(vec![])
    }
    fn columns(&self, _: &str) -> Result<Vec<ColumnDef>, DbError> {
        Ok(vec![])
    }
    fn indexes(&self, _: &str) -> Result<Vec<IndexDef>, DbError> {
        Ok(vec![])
    }
    fn table_collation(&self, _: &str) -> Result<Option<String>, DbError> {
        Ok(None)
    }
}

struct Recording {
    upsert: UpsertForm,
    log: RefCell<Vec<String>>,
    params: RefCell<Vec<Vec<DbValue>>>,
    open: Cell<bool>,
}

impl Recording {
    fn new(upsert: UpsertForm) -> Rc<Recording> {
        Rc::new(Recording {
            upsert,
            log: RefCell::new(vec![]),
            params: RefCell::new(vec![]),
            open: Cell::new(false),
        })
    }
    fn record(&self, stmt: &Statement) {
        self.log.borrow_mut().push(stmt.tree().to_string());
        self.params.borrow_mut().push(stmt.params().to_vec());
    }
}

impl SqlStore for Recording {
    fn engine(&self) -> &str {
        "fake"
    }
    fn dialect(&self) -> &dyn Dialect {
        &NoDialect
    }
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            transactions: true,
            savepoints: true,
            ordered_ranges: true,
            expiry: false,
            atomic_increment: false,
            returning: false,
            upsert: self.upsert,
        }
    }
    fn execute(&self, stmt: &Statement) -> Result<ExecResult, DbError> {
        self.record(stmt);
        Ok(ExecResult {
            rows_affected: 1,
            insert_id: None,
        })
    }
    fn query(&self, stmt: &Statement) -> Result<Rows, DbError> {
        self.record(stmt);
        Ok(Rows::default())
    }
    fn begin(&self) -> Result<(), DbError> {
        self.log.borrow_mut().push("begin".into());
        self.open.set(true);
        Ok(())
    }
    fn commit(&self) -> Result<(), DbError> {
        self.log.borrow_mut().push("commit".into());
        self.open.set(false);
        Ok(())
    }
    fn rollback(&self) -> Result<(), DbError> {
        self.log.borrow_mut().push("rollback".into());
        self.open.set(false);
        Ok(())
    }
    fn savepoint(&self, _: &str) -> Result<(), DbError> {
        Ok(())
    }
    fn release_savepoint(&self, _: &str) -> Result<(), DbError> {
        Ok(())
    }
    fn rollback_to_savepoint(&self, _: &str) -> Result<(), DbError> {
        Ok(())
    }
    fn in_transaction(&self) -> bool {
        self.open.get()
    }
    fn introspect(&self) -> &dyn Introspector {
        &NoIntrospector
    }
    fn reset(&self) -> Result<(), DbError> {
        Ok(())
    }
}

#[test]
fn set_emits_upsert() {
    let store = Recording::new(UpsertForm::OnDuplicateKey);
    let cache = SqlCache::new(store.clone(), "wp_speedyspec_cache").unwrap();
    cache.set("g", "k", b"v", None).unwrap();
    let log = store.log.borrow();
    let last = log.last().unwrap();
    assert!(
        last.contains("ON DUPLICATE KEY UPDATE cache_value = VALUES(cache_value)"),
        "{last}"
    );
    assert_eq!(
        store.params.borrow().last().unwrap(),
        &vec![
            DbValue::Bytes(b"g".to_vec()),
            DbValue::Bytes(b"k".to_vec()),
            DbValue::Bytes(b"v".to_vec()),
            DbValue::Null
        ]
    );
    assert_eq!(cache.name(), "sql:fake");
}

#[test]
fn set_without_upsert_deletes_then_inserts() {
    let store = Recording::new(UpsertForm::None);
    let cache = SqlCache::new(store.clone(), "wp_speedyspec_cache").unwrap();
    cache.set("g", "k", b"v", None).unwrap();
    let log = store.log.borrow();
    assert_eq!(log.len(), 4, "{log:?}");
    assert_eq!(log[0], "begin");
    assert!(log[1].starts_with("DELETE"), "{}", log[1]);
    assert!(log[2].starts_with("INSERT"), "{}", log[2]);
    assert_eq!(log[3], "commit");
}

#[test]
fn stored_key_digest_is_pinned() {
    // `printf 'a%.0s' {1..192} | shasum -a 256`
    assert_eq!(
        stored_key(&"a".repeat(192)),
        "sha256:7cee24628d290c16183532716cc5a8a889bc951b4b0a1507c32b8e29cee01052"
    );
}

#[test]
fn stored_key_hashes_long_keys() {
    let k191 = "a".repeat(191);
    assert_eq!(stored_key(&k191), k191);
    let long = stored_key(&"a".repeat(192));
    assert!(
        long.starts_with("sha256:") && long.len() == 7 + 64,
        "{long}"
    );
    assert!(
        long[7..]
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
    );
    // Bytes, not characters (the columns are varbinary): 95 two-byte characters stay, 96 are hashed.
    assert_eq!(stored_key(&"é".repeat(95)), "é".repeat(95));
    assert!(stored_key(&"é".repeat(96)).starts_with("sha256:"));
}
