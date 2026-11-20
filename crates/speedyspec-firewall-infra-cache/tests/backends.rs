use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use speedyspec_cache::{CacheError, CachingStore};
use speedyspec_cache_file::FileCache;
use speedyspec_cache_kv::KvCache;
use speedyspec_cache_memory::MemoryCache;
use speedyspec_data::DirectStore;
use speedyspec_data_file::FileStore;
use speedyspec_data_memory::MemoryNoSqlStore;
use speedyspec_firewall::{BanStore, Clock, CounterStore};
use speedyspec_firewall_infra_cache::{CacheBanStore, CacheCounterStore};

struct FixedClock(Cell<SystemTime>);

impl Clock for FixedClock {
    fn now(&self) -> SystemTime {
        self.0.get()
    }
}

fn clock() -> Rc<FixedClock> {
    Rc::new(FixedClock(Cell::new(UNIX_EPOCH + Duration::from_secs(1_800_000_000))))
}

fn backends(dir: &std::path::Path) -> Vec<(&'static str, Rc<dyn CachingStore>)> {
    let files = FileStore::new("files", dir.to_path_buf());
    files.connect().unwrap();
    vec![
        ("memory", Rc::new(MemoryCache::new())),
        ("kv", Rc::new(KvCache::new(Rc::new(MemoryNoSqlStore::new("memory")), "fw"))),
        ("file", Rc::new(FileCache::new(&files, "fw").unwrap())),
    ]
}

fn counts_within_window_and_resets_in_next(cache: Rc<dyn CachingStore>) {
    let clock = clock();
    let counters = CacheCounterStore::new(cache, clock.clone());
    let window = Duration::from_secs(60);
    assert_eq!(counters.hit("rate:192.0.2.1", window).unwrap(), 1);
    assert_eq!(counters.hit("rate:192.0.2.1", window).unwrap(), 2);
    assert_eq!(counters.hit("rate:192.0.2.1", window).unwrap(), 3);
    assert_eq!(counters.hit("rate:192.0.2.2", window).unwrap(), 1);
    clock.0.set(clock.0.get() + window);
    assert_eq!(counters.hit("rate:192.0.2.1", window).unwrap(), 1);
}

fn lift_removes_ban(cache: Rc<dyn CachingStore>) {
    let bans = CacheBanStore::new(cache);
    bans.ban("ip:x", Duration::from_secs(60), "rate limit").unwrap();
    assert_eq!(bans.banned("ip:x").unwrap().as_deref(), Some("rate limit"));
    assert_eq!(bans.banned("ip:y").unwrap(), None);
    bans.lift("ip:x").unwrap();
    assert_eq!(bans.banned("ip:x").unwrap(), None);
}

#[test]
fn counts_within_window_and_resets_in_next_on_every_backend() {
    let dir = tempfile::tempdir().unwrap();
    for (name, cache) in backends(dir.path()) {
        eprintln!("backend: {name}");
        counts_within_window_and_resets_in_next(cache);
    }
}

#[test]
fn lift_removes_ban_on_every_backend() {
    let dir = tempfile::tempdir().unwrap();
    for (name, cache) in backends(dir.path()) {
        eprintln!("backend: {name}");
        lift_removes_ban(cache);
    }
}

/// Wraps a cache, records `add` calls and `set` expiries, and can make the first `increment` find nothing.
struct Recording {
    inner: MemoryCache,
    adds: Cell<u32>,
    lose_first_increment: Cell<bool>,
    set_expiry: RefCell<Option<Duration>>,
}

impl Recording {
    fn new(lose_first_increment: bool) -> Recording {
        Recording {
            inner: MemoryCache::new(),
            adds: Cell::new(0),
            lose_first_increment: Cell::new(lose_first_increment),
            set_expiry: RefCell::new(None),
        }
    }
}

impl CachingStore for Recording {
    fn name(&self) -> &str {
        "recording"
    }
    fn get(&self, group: &str, key: &str) -> Result<Option<Vec<u8>>, CacheError> {
        self.inner.get(group, key)
    }
    fn get_many(&self, group: &str, keys: &[&str]) -> Result<Vec<Option<Vec<u8>>>, CacheError> {
        self.inner.get_many(group, keys)
    }
    fn set(&self, group: &str, key: &str, value: &[u8], expiry: Option<Duration>) -> Result<(), CacheError> {
        *self.set_expiry.borrow_mut() = expiry;
        self.inner.set(group, key, value, expiry)
    }
    fn add(&self, group: &str, key: &str, value: &[u8], expiry: Option<Duration>) -> Result<bool, CacheError> {
        self.adds.set(self.adds.get() + 1);
        self.inner.add(group, key, value, expiry)
    }
    fn replace(&self, group: &str, key: &str, value: &[u8], expiry: Option<Duration>) -> Result<bool, CacheError> {
        self.inner.replace(group, key, value, expiry)
    }
    fn delete(&self, group: &str, key: &str) -> Result<bool, CacheError> {
        self.inner.delete(group, key)
    }
    fn increment(&self, group: &str, key: &str, by: i64) -> Result<Option<i64>, CacheError> {
        if self.lose_first_increment.replace(false) {
            self.inner.delete(group, key)?;
            return Ok(None);
        }
        self.inner.increment(group, key, by)
    }
    fn decrement(&self, group: &str, key: &str, by: i64) -> Result<Option<i64>, CacheError> {
        self.inner.decrement(group, key, by)
    }
    fn flush(&self) -> Result<(), CacheError> {
        self.inner.flush()
    }
    fn flush_group(&self, group: &str) -> Result<(), CacheError> {
        self.inner.flush_group(group)
    }
}

#[test]
fn evicted_counter_is_recreated() {
    let cache = Rc::new(Recording::new(true));
    let counters = CacheCounterStore::new(cache.clone(), clock());
    assert_eq!(counters.hit("rate:192.0.2.1", Duration::from_secs(60)).unwrap(), 1);
    assert_eq!(cache.adds.get(), 2);
}

#[test]
fn ban_reason_round_trips_with_its_expiry() {
    let cache = Rc::new(Recording::new(false));
    let bans = CacheBanStore::new(cache.clone());
    bans.ban("ip:x", Duration::from_secs(60), "rate limit").unwrap();
    assert_eq!(bans.banned("ip:x").unwrap().as_deref(), Some("rate limit"));
    assert_eq!(*cache.set_expiry.borrow(), Some(Duration::from_secs(60)));
}

#[test]
#[ignore = "needs Valkey: set SPEEDYSPEC_TEST_VALKEY_URL"]
fn valkey_counts() {
    use speedyspec_data_valkey::{ValkeyStore, driver_url};
    let url = std::env::var("SPEEDYSPEC_TEST_VALKEY_URL").expect("SPEEDYSPEC_TEST_VALKEY_URL");
    let prefix = format!("speedyspec-firewall-test-{}:", std::process::id());
    let store = ValkeyStore::new("valkey", &driver_url(&url, false).unwrap(), &prefix, Duration::from_secs(2)).unwrap();
    let cache: Rc<dyn CachingStore> = Rc::new(KvCache::new(Rc::new(store), "fw"));
    cache.flush_group("speedyspec-firewall").unwrap();
    counts_within_window_and_resets_in_next(cache.clone());
    lift_removes_ban(cache);
}
