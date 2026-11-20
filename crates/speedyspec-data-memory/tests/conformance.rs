use std::collections::BTreeMap;
use std::rc::Rc;

use speedyspec_data::EngineConfig;
use speedyspec_data_conformance::{direct_store_suite, nosql_store_suite};
use speedyspec_data_memory::MemoryEngineFactory;
use speedyspec_data_query::EngineRegistry;

fn registry(url: &str) -> EngineRegistry {
    let registry = EngineRegistry::new();
    registry.register_factory(Rc::new(MemoryEngineFactory));
    registry
        .configure(EngineConfig {
            name: "mem".into(),
            url: url.into(),
            options: BTreeMap::new(),
        })
        .unwrap();
    registry.route("test", "mem").unwrap();
    registry
}

#[test]
fn memory_nosql_conformance() {
    let store = registry("memory://").nosql("test").unwrap();
    nosql_store_suite(&*store, "conf");
}

#[test]
fn memory_direct_conformance() {
    let store = registry("memory://direct").direct("test").unwrap();
    direct_store_suite(&*store, "memory");
}

#[test]
fn huge_ttl_is_kept() {
    use speedyspec_data::NoSqlStore;
    let store = speedyspec_data_memory::MemoryNoSqlStore::new("memory");
    store
        .put(
            "ks",
            b"far",
            b"x",
            Some(std::time::Duration::from_secs(i64::MAX as u64)),
        )
        .unwrap();
    assert_eq!(
        store.get("ks", b"far").unwrap().as_deref(),
        Some(b"x".as_slice())
    );
}
