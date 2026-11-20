use std::collections::BTreeMap;
use std::rc::Rc;
use std::time::Duration;

use speedyspec_cache::CachingStore;
use speedyspec_cache_conformance::caching_store_suite;
use speedyspec_cache_sql::SqlCache;
use speedyspec_data::{EngineConfig, SqlStore};
use speedyspec_data_mysql::MySqlEngineFactory;
use speedyspec_data_query::EngineRegistry;

fn store() -> Rc<dyn SqlStore> {
    let url = std::env::var("SPEEDYSPEC_TEST_MYSQL_URL").expect("set SPEEDYSPEC_TEST_MYSQL_URL");
    let registry = EngineRegistry::new();
    registry.register_factory(Rc::new(MySqlEngineFactory));
    registry
        .configure(EngineConfig {
            name: "default".into(),
            url,
            options: BTreeMap::new(),
        })
        .unwrap();
    registry.sql("default").unwrap()
}

#[test]
#[ignore = "needs MySQL: set SPEEDYSPEC_TEST_MYSQL_URL"]
fn sql_cache_conformance() {
    let store = store();
    // A fresh table each run, so a change to the schema is what the suite checks.
    store
        .execute(
            &speedyspec_data_query::parse_one("DROP TABLE IF EXISTS speedyspec_cache_conformance")
                .unwrap(),
        )
        .unwrap();
    let cache = SqlCache::new(store, "speedyspec_cache_conformance").unwrap();
    cache.install().unwrap();
    cache.install().unwrap();
    caching_store_suite(&cache);
}

#[test]
#[ignore = "needs MySQL: set SPEEDYSPEC_TEST_MYSQL_URL"]
fn purge_removes_only_expired() {
    let cache = SqlCache::new(store(), "speedyspec_cache_purge").unwrap();
    cache.install().unwrap();
    cache.flush().unwrap();
    cache.set("g", "gone", b"1", Some(Duration::ZERO)).unwrap();
    cache.set("g", "kept", b"2", None).unwrap();
    cache
        .set("g", "later", b"3", Some(Duration::from_secs(600)))
        .unwrap();
    assert_eq!(cache.purge_expired().unwrap(), 1);
    assert_eq!(cache.get("g", "kept").unwrap().as_deref(), Some(&b"2"[..]));
    assert_eq!(cache.get("g", "later").unwrap().as_deref(), Some(&b"3"[..]));
}
