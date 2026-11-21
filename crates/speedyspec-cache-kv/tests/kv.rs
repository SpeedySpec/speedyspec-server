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

#[test]
fn flush_group_removes_old_generations_on_ordered_stores() {
    let store = memory();
    let cache = KvCache::new(store.clone(), "cache");
    for round in 0..3 {
        for i in 0..100 {
            cache
                .set("posts", &format!("{round}-{i}"), b"v", None)
                .unwrap();
        }
        cache.set("users", "u", b"kept", None).unwrap();
        cache.flush_group("posts").unwrap();
    }
    let all = store
        .range(
            "cache",
            std::ops::Bound::Unbounded,
            std::ops::Bound::Unbounded,
            None,
        )
        .unwrap();
    // Only the two generation counters and the users entry remain.
    assert_eq!(all.len(), 2, "{} entries left", all.len());
    assert_eq!(
        cache.get("users", "u").unwrap().as_deref(),
        Some(b"kept".as_slice())
    );
}

/// A store that deletes an entry right after it is read, as another process's `delete` landing between two calls.
struct DeletedAfterRead {
    inner: Rc<dyn NoSqlStore>,
    entry: Vec<u8>,
}

impl NoSqlStore for DeletedAfterRead {
    fn engine(&self) -> &str {
        self.inner.engine()
    }
    fn capabilities(&self) -> speedyspec_data::Capabilities {
        self.inner.capabilities()
    }
    fn get(&self, ks: &str, key: &[u8]) -> Result<Option<Vec<u8>>, speedyspec_data::DbError> {
        let got = self.inner.get(ks, key)?;
        if key == self.entry.as_slice() {
            self.inner.delete(ks, key)?;
        }
        Ok(got)
    }
    fn get_many(
        &self,
        ks: &str,
        keys: &[&[u8]],
    ) -> Result<Vec<Option<Vec<u8>>>, speedyspec_data::DbError> {
        self.inner.get_many(ks, keys)
    }
    fn put(
        &self,
        ks: &str,
        key: &[u8],
        value: &[u8],
        ttl: Option<Duration>,
    ) -> Result<(), speedyspec_data::DbError> {
        self.inner.put(ks, key, value, ttl)
    }
    fn put_if_absent(
        &self,
        ks: &str,
        key: &[u8],
        value: &[u8],
        ttl: Option<Duration>,
    ) -> Result<bool, speedyspec_data::DbError> {
        self.inner.put_if_absent(ks, key, value, ttl)
    }
    fn delete(&self, ks: &str, key: &[u8]) -> Result<bool, speedyspec_data::DbError> {
        self.inner.delete(ks, key)
    }
    fn range(
        &self,
        ks: &str,
        start: std::ops::Bound<&[u8]>,
        end: std::ops::Bound<&[u8]>,
        limit: Option<usize>,
    ) -> Result<speedyspec_data::KeyValues, speedyspec_data::DbError> {
        self.inner.range(ks, start, end, limit)
    }
    fn increment(&self, ks: &str, key: &[u8], by: i64) -> Result<i64, speedyspec_data::DbError> {
        self.inner.increment(ks, key, by)
    }
    fn adjust_existing(
        &self,
        ks: &str,
        key: &[u8],
        by: i64,
        floor: Option<i64>,
    ) -> Result<Option<i64>, speedyspec_data::DbError> {
        self.inner.adjust_existing(ks, key, by, floor)
    }
    fn clear(&self, ks: &str) -> Result<(), speedyspec_data::DbError> {
        self.inner.clear(ks)
    }
}

/// An entry deleted while it is being incremented never comes back holding the increment.
#[test]
fn increment_never_recreates_a_deleted_entry() {
    let inner = memory();
    let store = Rc::new(DeletedAfterRead {
        inner: inner.clone(),
        entry: keys::entry(b"g", 0, b"n"),
    });
    let cache = KvCache::new(store, "cache");
    KvCache::new(inner.clone(), "cache")
        .set("g", "n", b"10", None)
        .unwrap();
    let got = cache.increment("g", "n", 5).unwrap();
    let stored = KvCache::new(inner, "cache").get("g", "n").unwrap();
    assert!(
        stored.as_deref() != Some(b"5".as_slice()),
        "the deleted entry came back holding the increment: got {got:?}"
    );
}
