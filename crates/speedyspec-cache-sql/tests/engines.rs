//! `SqlCache` on SQLite and PostgreSQL, and `add` inside a caller's transaction.

use std::collections::BTreeMap;
use std::rc::Rc;

use speedyspec_cache::CachingStore;
use speedyspec_cache_conformance::caching_store_suite;
use speedyspec_cache_sql::SqlCache;
use speedyspec_data::{EngineConfig, EngineFactory, SqlStore};
use speedyspec_data_postgres::PostgresEngineFactory;
use speedyspec_data_query::{EngineRegistry, parse_one};
use speedyspec_data_sqlite::SqliteEngineFactory;

fn store(
    factory: Rc<dyn EngineFactory>,
    url: String,
    options: &[(&str, &str)],
) -> Rc<dyn SqlStore> {
    let registry = EngineRegistry::new();
    registry.register_factory(factory);
    registry
        .configure(EngineConfig {
            name: "default".into(),
            url,
            options: options
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect::<BTreeMap<_, _>>(),
        })
        .unwrap();
    registry.sql("default").unwrap()
}

fn sqlite() -> Rc<dyn SqlStore> {
    store(
        Rc::new(SqliteEngineFactory),
        "sqlite://:memory:".into(),
        &[],
    )
}

/// A store on its own schema `schema`, made afresh (the tests run in parallel).
fn postgres(schema: &str) -> Rc<dyn SqlStore> {
    let url =
        std::env::var("SPEEDYSPEC_TEST_POSTGRES_URL").expect("set SPEEDYSPEC_TEST_POSTGRES_URL");
    let mut admin = postgres::Client::connect(&url, postgres::NoTls).expect("PostgreSQL is up");
    admin
        .batch_execute(&format!(
            "DROP SCHEMA IF EXISTS {schema} CASCADE; CREATE SCHEMA {schema}"
        ))
        .unwrap();
    store(Rc::new(PostgresEngineFactory), url, &[("schema", schema)])
}

fn cache(store: Rc<dyn SqlStore>) -> SqlCache {
    let cache = SqlCache::new(store, "speedyspec_cache").unwrap();
    cache.install().unwrap();
    cache
}

/// A second `add` of a key answers false without an error, so a transaction the caller holds stays usable (a
/// constraint error would abort it on PostgreSQL).
fn add_keeps_the_callers_transaction(store: Rc<dyn SqlStore>) {
    let cache = cache(store.clone());
    store.begin().unwrap();
    assert!(cache.add("g", "k", b"1", None).unwrap());
    assert!(!cache.add("g", "k", b"2", None).unwrap());
    assert_eq!(cache.get("g", "k").unwrap().as_deref(), Some(&b"1"[..]));
    store
        .execute(&parse_one("SELECT 1").unwrap())
        .expect("the transaction is still usable");
    store.commit().unwrap();
    assert_eq!(cache.get("g", "k").unwrap().as_deref(), Some(&b"1"[..]));
}

#[test]
fn sqlite_conformance() {
    caching_store_suite(&cache(sqlite()));
}

#[test]
fn sqlite_add_keeps_the_callers_transaction() {
    add_keeps_the_callers_transaction(sqlite());
}

#[test]
#[ignore = "needs PostgreSQL: set SPEEDYSPEC_TEST_POSTGRES_URL"]
fn postgres_conformance() {
    caching_store_suite(&cache(postgres("t_sql_cache_suite")));
}

#[test]
#[ignore = "needs PostgreSQL: set SPEEDYSPEC_TEST_POSTGRES_URL"]
fn postgres_add_keeps_the_callers_transaction() {
    add_keeps_the_callers_transaction(postgres("t_sql_cache_add"));
}
