use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::rc::Rc;

use speedyspec_cache::CachingStore;
use speedyspec_cache_routing::{OBJECT_CACHE_USE, PersistentCacheSelector};
use speedyspec_data::*;
use speedyspec_data_file::FileStore;
use speedyspec_data_memory::{MemoryDirectStore, MemoryNoSqlStore};
use speedyspec_data_query::EngineRegistry;

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

/// An engine that hands out one fixed store, so a test can see what reached it.
struct Fixed {
    name: String,
    handle: EngineHandle,
}
impl Engine for Fixed {
    fn name(&self) -> &str {
        &self.name
    }
    fn kind(&self) -> EngineKind {
        EngineKind::Memory
    }
    fn open(&self) -> Result<EngineHandle, DbError> {
        Ok(self.handle.clone())
    }
}

struct FixedFactory {
    scheme: &'static str,
    handle: EngineHandle,
}
impl EngineFactory for FixedFactory {
    fn scheme(&self) -> &'static str {
        self.scheme
    }
    fn create(&self, config: EngineConfig) -> Result<Rc<dyn Engine>, DbError> {
        Ok(Rc::new(Fixed {
            name: config.name,
            handle: self.handle.clone(),
        }))
    }
}

/// A registry with one engine, `engine`, of `scheme`, giving `handle`; `default` is the same engine.
fn registry(scheme: &'static str, engine: &str, handle: EngineHandle) -> EngineRegistry {
    let registry = EngineRegistry::new();
    registry.register_factory(Rc::new(FixedFactory { scheme, handle }));
    let names = if engine == "default" {
        vec!["default"]
    } else {
        vec!["default", engine]
    };
    for name in names {
        registry
            .configure(EngineConfig {
                name: name.into(),
                url: format!("{scheme}://x"),
                options: BTreeMap::new(),
            })
            .unwrap();
    }
    registry
}

fn select(registry: &EngineRegistry) -> Result<Option<Rc<dyn CachingStore>>, DbError> {
    PersistentCacheSelector::new().select(registry, OBJECT_CACHE_USE)
}

#[test]
fn unrouted_is_none() {
    let registry = registry(
        "fkv",
        "kv",
        EngineHandle::NoSql(Rc::new(MemoryNoSqlStore::new("kv"))),
    );
    assert!(select(&registry).unwrap().is_none());
}

#[test]
fn sql_engine_gives_sql_cache() {
    let store = Recording::new(UpsertForm::OnDuplicateKey);
    let registry = registry("fsql", "s", EngineHandle::Sql(store.clone()));
    registry.route(OBJECT_CACHE_USE, "s").unwrap();
    let cache = select(&registry).unwrap().expect("persistent");
    cache.set("g", "k", b"v", None).unwrap();
    assert!(
        store
            .log
            .borrow()
            .iter()
            .any(|s| s.contains("speedyspec_cache")),
        "{:?}",
        store.log.borrow()
    );
}

#[test]
fn nosql_engine_gives_kv_cache() {
    let store = Rc::new(MemoryNoSqlStore::new("kv"));
    let registry = registry("fkv", "kv", EngineHandle::NoSql(store.clone()));
    registry.route(OBJECT_CACHE_USE, "kv").unwrap();
    let cache = select(&registry).unwrap().expect("persistent");
    cache.set("g", "k", b"v", None).unwrap();
    let stored = store
        .range(
            "object-cache",
            std::ops::Bound::Unbounded,
            std::ops::Bound::Unbounded,
            None,
        )
        .unwrap();
    assert!(stored.iter().any(|(_, v)| v == b"v"), "{stored:?}");
}

#[test]
fn file_engine_gives_file_cache() {
    let dir = tempfile::tempdir().unwrap();
    let store = FileStore::new("f", dir.path().to_path_buf());
    store.connect().unwrap();
    let registry = registry("ffile", "f", EngineHandle::Direct(Rc::new(store)));
    registry.route(OBJECT_CACHE_USE, "f").unwrap();
    let cache = select(&registry).unwrap().expect("persistent");
    cache.set("g", "k", b"v", None).unwrap();
    assert!(dir.path().join("object-cache").is_dir());
    assert_eq!(
        cache.get("g", "k").unwrap().as_deref(),
        Some(b"v".as_slice())
    );
}

#[test]
fn other_direct_is_wrong_kind() {
    let registry = registry(
        "fdirect",
        "d",
        EngineHandle::Direct(Rc::new(MemoryDirectStore::new("d"))),
    );
    registry.route(OBJECT_CACHE_USE, "d").unwrap();
    assert!(matches!(select(&registry), Err(DbError::WrongKind { engine, .. }) if engine == "d"));
}

#[test]
fn routing_to_default_is_persistent() {
    let registry = registry(
        "fkv",
        "default",
        EngineHandle::NoSql(Rc::new(MemoryNoSqlStore::new("kv"))),
    );
    registry.route(OBJECT_CACHE_USE, "default").unwrap();
    assert!(select(&registry).unwrap().is_some());
}
