use std::rc::Rc;
use std::time::Duration;

use speedyspec_cache::CachingStore;
use speedyspec_cache_conformance::caching_store_suite;
use speedyspec_cache_kv::{KvCache, keys};
use speedyspec_data::NoSqlStore;
use speedyspec_data_memory::MemoryNoSqlStore;
use speedyspec_data_redb::{OwnerConfig, RedbClient, RedbOwner};
use speedyspec_data_valkey::{ValkeyStore, driver_url};

fn memory() -> Rc<dyn NoSqlStore> {
    Rc::new(MemoryNoSqlStore::new("memory"))
}

#[test]
fn kv_cache_on_memory() {
    caching_store_suite(&KvCache::new(memory(), "cache"));
}

#[test]
fn kv_cache_on_redb() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = OwnerConfig {
        engine: "redb".into(),
        file: dir.path().join("site.redb"),
        socket: dir.path().join("site.owner/owner.sock"),
        sweep_every: Duration::from_secs(60),
    };
    let owner = RedbOwner::open(cfg.clone()).unwrap();
    let stop = owner.stop_handle();
    let thread = std::thread::spawn(move || owner.serve());
    let client = RedbClient::new("redb", cfg.socket.clone(), Duration::from_secs(5)).unwrap();
    caching_store_suite(&KvCache::new(Rc::new(client), "cache"));
    stop.stop();
    thread.join().unwrap().unwrap();
}

#[test]
#[ignore = "needs Valkey: set SPEEDYSPEC_TEST_VALKEY_URL"]
fn kv_cache_on_valkey() {
    let url = std::env::var("SPEEDYSPEC_TEST_VALKEY_URL").expect("SPEEDYSPEC_TEST_VALKEY_URL");
    let prefix = format!("speedyspec-kv-test-{}:", std::process::id());
    let store = ValkeyStore::new(
        "valkey",
        &driver_url(&url, false).unwrap(),
        &prefix,
        Duration::from_secs(2),
    )
    .unwrap();
    caching_store_suite(&KvCache::new(Rc::new(store), "cache"));
}

#[test]
fn flush_group_is_seen_by_another_instance() {
    let store = memory();
    let (a, b) = (
        KvCache::new(store.clone(), "cache"),
        KvCache::new(store, "cache"),
    );
    a.set("posts", "k", b"p", None).unwrap();
    a.set("users", "k", b"u", None).unwrap();
    b.flush_group("posts").unwrap();
    assert_eq!(a.get("posts", "k").unwrap(), None);
    assert_eq!(
        a.get("users", "k").unwrap().as_deref(),
        Some(b"u".as_slice())
    );
}

#[test]
fn group_and_key_are_unambiguous() {
    let cache = KvCache::new(memory(), "cache");
    cache.set("a", "bc", b"1", None).unwrap();
    cache.set("ab", "c", b"2", None).unwrap();
    assert_eq!(
        cache.get("a", "bc").unwrap().as_deref(),
        Some(b"1".as_slice())
    );
    assert_eq!(
        cache.get("ab", "c").unwrap().as_deref(),
        Some(b"2".as_slice())
    );
}

#[test]
fn entry_key_layout() {
    assert_eq!(
        keys::entry(b"g", 7, b"k"),
        vec![b'e', 0, 0, 0, 1, b'g', 0, 0, 0, 0, 0, 0, 0, 7, b'k']
    );
}
