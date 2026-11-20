use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use speedyspec_cache::{CacheError, CachingStore};
use speedyspec_cache_memory::MemoryCache;
use speedyspec_hooks::{Key, Value};
use speedyspec_wp_object_cache::*;

#[derive(Default)]
struct Misses(RefCell<Vec<(String, String)>>);
impl CacheNotices for Misses {
    fn doing_it_wrong(&self, _: &str, _: &str, _: &str) {}
    fn deprecated_function(&self, _: &str, _: &str, _: &str) {}
    fn persistence_miss(&self, group: &str, key: &str, _: &str) {
        self.0.borrow_mut().push((group.into(), key.into()));
    }
}

struct Multisite;
impl SiteContext for Multisite {
    fn is_multisite(&self) -> bool {
        true
    }
    fn current_blog_id(&self) -> i64 {
        2
    }
}

fn s(v: &str) -> Value {
    Value::Str(v.into())
}

fn cache_over(store: Rc<dyn CachingStore>) -> (ObjectCache, Rc<Misses>) {
    let misses = Rc::new(Misses::default());
    let mut ports = ObjectCachePorts::rust();
    ports.notices = misses.clone();
    (ObjectCache::new(ports, Some(store)), misses)
}

fn rig() -> (ObjectCache, Rc<MemoryCache>, Rc<Misses>) {
    let store = Rc::new(MemoryCache::new());
    let (cache, misses) = cache_over(store.clone());
    (cache, store, misses)
}

#[test]
fn writes_php_serialize_format() {
    let (c, store, _) = rig();
    c.set(
        &s("k"),
        Value::Array(vec![(Key::Int(0), s("a"))]),
        &s("g"),
        0,
    )
    .unwrap();
    assert_eq!(
        store.get("g", "k").unwrap(),
        Some(b"a:1:{i:0;s:1:\"a\";}".to_vec())
    );
}

#[test]
fn reads_second_level_after_flush_runtime() {
    let (c, _, _) = rig();
    c.set(&s("k"), Value::Int(5), &s("g"), 0).unwrap();
    c.flush_runtime();
    assert_eq!(c.get(&s("k"), &s("g"), false).unwrap(), Some(Value::Int(5)));
    assert_eq!(c.hits(), 1);
    assert_eq!(c.misses(), 0);
}

#[test]
fn non_persistent_groups_stay_first_level() {
    let (c, store, _) = rig();
    c.add_non_persistent_groups(&[s("counts")]);
    c.set(&s("k"), Value::Int(1), &s("counts"), 0).unwrap();
    assert_eq!(store.get("counts", "k").unwrap(), None);
    assert_eq!(
        c.get(&s("k"), &s("counts"), false).unwrap(),
        Some(Value::Int(1))
    );
}

#[test]
fn refused_value_stays_first_level() {
    let (c, store, misses) = rig();
    let opaque = Rc::new(5u8);
    assert!(
        c.set(&s("k"), Value::Opaque(opaque.clone()), &s("g"), 0)
            .unwrap()
    );
    match c.get(&s("k"), &s("g"), false).unwrap() {
        Some(Value::Opaque(back)) => assert!(Rc::ptr_eq(
            &(back as Rc<dyn std::any::Any>),
            &(opaque.clone() as Rc<dyn std::any::Any>)
        )),
        other => panic!("{other:?}"),
    }
    assert_eq!(store.get("g", "k").unwrap(), None);
    assert_eq!(*misses.0.borrow(), vec![("g".to_string(), "k".to_string())]);
}

#[test]
fn blog_prefix_in_second_level_key() {
    let store = Rc::new(MemoryCache::new());
    let mut ports = ObjectCachePorts::rust();
    ports.site = Rc::new(Multisite);
    let c = ObjectCache::new(ports, Some(store.clone()));
    c.set(&s("k"), Value::Int(1), &s("posts"), 0).unwrap();
    assert_eq!(store.get("posts", "2:k").unwrap(), Some(b"i:1;".to_vec()));
}

#[test]
fn add_sees_another_worker() {
    let (c, store, _) = rig();
    store.set("g", "k", b"i:1;", None).unwrap();
    assert!(!c.add(&s("k"), Value::Int(2), &s("g"), 0).unwrap());
    assert!(c.replace(&s("k"), Value::Int(3), &s("g"), 0).unwrap());
    assert_eq!(store.get("g", "k").unwrap(), Some(b"i:3;".to_vec()));
}

#[test]
fn delete_flush_group_flush_reach_second_level() {
    let (c, store, _) = rig();
    store.set("g", "k", b"i:1;", None).unwrap();
    assert!(c.delete(&s("k"), &s("g")).unwrap());
    assert_eq!(store.get("g", "k").unwrap(), None);
    c.set(&s("a"), Value::Int(1), &s("g"), 0).unwrap();
    c.set(&s("b"), Value::Int(1), &s("h"), 0).unwrap();
    c.flush_group(&s("g")).unwrap();
    assert_eq!(store.get("g", "a").unwrap(), None);
    assert!(store.get("h", "b").unwrap().is_some());
    c.flush();
    assert_eq!(store.get("h", "b").unwrap(), None);
}

/// Records the expiry each `set` asks for.
struct Expiries {
    inner: MemoryCache,
    seen: RefCell<Vec<Option<Duration>>>,
}
impl CachingStore for Expiries {
    fn name(&self) -> &str {
        "expiries"
    }
    fn get(&self, g: &str, k: &str) -> Result<Option<Vec<u8>>, CacheError> {
        self.inner.get(g, k)
    }
    fn get_many(&self, g: &str, k: &[&str]) -> Result<Vec<Option<Vec<u8>>>, CacheError> {
        self.inner.get_many(g, k)
    }
    fn set(&self, g: &str, k: &str, v: &[u8], e: Option<Duration>) -> Result<(), CacheError> {
        self.seen.borrow_mut().push(e);
        self.inner.set(g, k, v, e)
    }
    fn add(&self, g: &str, k: &str, v: &[u8], e: Option<Duration>) -> Result<bool, CacheError> {
        self.inner.add(g, k, v, e)
    }
    fn replace(&self, g: &str, k: &str, v: &[u8], e: Option<Duration>) -> Result<bool, CacheError> {
        self.inner.replace(g, k, v, e)
    }
    fn delete(&self, g: &str, k: &str) -> Result<bool, CacheError> {
        self.inner.delete(g, k)
    }
    fn increment(&self, g: &str, k: &str, by: i64) -> Result<Option<i64>, CacheError> {
        self.inner.increment(g, k, by)
    }
    fn decrement(&self, g: &str, k: &str, by: i64) -> Result<Option<i64>, CacheError> {
        self.inner.decrement(g, k, by)
    }
    fn flush(&self) -> Result<(), CacheError> {
        self.inner.flush()
    }
    fn flush_group(&self, g: &str) -> Result<(), CacheError> {
        self.inner.flush_group(g)
    }
}

#[test]
fn expire_becomes_duration() {
    let store = Rc::new(Expiries {
        inner: MemoryCache::new(),
        seen: RefCell::new(vec![]),
    });
    let (c, _) = cache_over(store.clone());
    c.set(&s("a"), Value::Int(1), &s("g"), 30).unwrap();
    c.set(&s("b"), Value::Int(1), &s("g"), 0).unwrap();
    c.set(&s("c"), Value::Int(1), &s("g"), -1).unwrap();
    assert_eq!(
        *store.seen.borrow(),
        vec![Some(Duration::from_secs(30)), None, None]
    );
}

#[test]
fn incr_writes_serialized() {
    let (c, store, _) = rig();
    c.set(&s("n"), Value::Int(5), &s("g"), 0).unwrap();
    assert_eq!(c.incr(&s("n"), 1, &s("g")).unwrap(), Some(Value::Int(6)));
    assert_eq!(store.get("g", "n").unwrap(), Some(b"i:6;".to_vec()));
}

#[test]
fn force_rereads_second_level() {
    let (c, store, _) = rig();
    c.set(&s("k"), Value::Int(1), &s("g"), 0).unwrap();
    store.set("g", "k", b"i:2;", None).unwrap();
    assert_eq!(c.get(&s("k"), &s("g"), false).unwrap(), Some(Value::Int(1)));
    assert_eq!(c.get(&s("k"), &s("g"), true).unwrap(), Some(Value::Int(2)));
}

#[test]
fn corrupt_second_level_is_a_miss() {
    let (c, store, misses) = rig();
    store.set("g", "k", b"a:1:{i:0;", None).unwrap();
    assert_eq!(c.get(&s("k"), &s("g"), false).unwrap(), None);
    assert_eq!(misses.0.borrow().len(), 1);
}

struct Failing;
impl CachingStore for Failing {
    fn name(&self) -> &str {
        "failing"
    }
    fn get(&self, _: &str, _: &str) -> Result<Option<Vec<u8>>, CacheError> {
        Err(CacheError::Backend {
            cache: "failing".into(),
            message: "down".into(),
        })
    }
    fn get_many(&self, _: &str, _: &[&str]) -> Result<Vec<Option<Vec<u8>>>, CacheError> {
        Err(CacheError::Backend {
            cache: "failing".into(),
            message: "down".into(),
        })
    }
    fn set(&self, _: &str, _: &str, _: &[u8], _: Option<Duration>) -> Result<(), CacheError> {
        Err(CacheError::Backend {
            cache: "failing".into(),
            message: "down".into(),
        })
    }
    fn add(&self, _: &str, _: &str, _: &[u8], _: Option<Duration>) -> Result<bool, CacheError> {
        Err(CacheError::Backend {
            cache: "failing".into(),
            message: "down".into(),
        })
    }
    fn replace(&self, _: &str, _: &str, _: &[u8], _: Option<Duration>) -> Result<bool, CacheError> {
        Err(CacheError::Backend {
            cache: "failing".into(),
            message: "down".into(),
        })
    }
    fn delete(&self, _: &str, _: &str) -> Result<bool, CacheError> {
        Err(CacheError::Backend {
            cache: "failing".into(),
            message: "down".into(),
        })
    }
    fn increment(&self, _: &str, _: &str, _: i64) -> Result<Option<i64>, CacheError> {
        Err(CacheError::Backend {
            cache: "failing".into(),
            message: "down".into(),
        })
    }
    fn decrement(&self, _: &str, _: &str, _: i64) -> Result<Option<i64>, CacheError> {
        Err(CacheError::Backend {
            cache: "failing".into(),
            message: "down".into(),
        })
    }
    fn flush(&self) -> Result<(), CacheError> {
        Err(CacheError::Backend {
            cache: "failing".into(),
            message: "down".into(),
        })
    }
    fn flush_group(&self, _: &str) -> Result<(), CacheError> {
        Err(CacheError::Backend {
            cache: "failing".into(),
            message: "down".into(),
        })
    }
}

#[test]
fn failing_store_is_a_miss() {
    let (c, misses) = cache_over(Rc::new(Failing));
    assert_eq!(c.get(&s("k"), &s("g"), false).unwrap(), None);
    assert!(c.set(&s("k"), Value::Int(1), &s("g"), 0).unwrap());
    assert_eq!(c.get(&s("k"), &s("g"), false).unwrap(), Some(Value::Int(1)));
    assert!(c.add(&s("new"), Value::Int(1), &s("g"), 0).unwrap());
    assert!(c.delete(&s("k"), &s("g")).unwrap());
    assert!(c.flush());
    assert!(!misses.0.borrow().is_empty());
}

#[test]
fn get_multiple_reads_misses_together() {
    let (c, store, _) = rig();
    store.set("g", "a", b"i:1;", None).unwrap();
    c.set(&s("b"), Value::Int(2), &s("g"), 0).unwrap();
    let got = c
        .get_multiple(&[s("a"), s("b"), s("none")], &s("g"), false)
        .unwrap();
    assert_eq!(
        got,
        vec![
            (Key::Str("a".into()), Value::Int(1)),
            (Key::Str("b".into()), Value::Int(2)),
            (Key::Str("none".into()), Value::Bool(false))
        ]
    );
}

#[test]
fn add_writes_through_the_second_levels_add() {
    let (c, store, _) = rig();
    assert!(c.add(&s("k"), Value::Int(4), &s("g"), 0).unwrap());
    assert_eq!(store.get("g", "k").unwrap(), Some(b"i:4;".to_vec()));
}

#[test]
fn refused_value_removes_the_persisted_one() {
    let (c, store, _) = rig();
    c.set(&s("k"), Value::Int(1), &s("g"), 0).unwrap();
    c.set(&s("k"), Value::Opaque(Rc::new(5u8)), &s("g"), 0)
        .unwrap();
    // The next request must not read the value the caller overwrote.
    let (next, _) = cache_over(store.clone());
    assert_eq!(next.get(&s("k"), &s("g"), false).unwrap(), None);
    assert_eq!(store.get("g", "k").unwrap(), None);
}

#[test]
fn corrupt_second_level_entry_is_removed() {
    let (c, store, _) = rig();
    store.set("g", "k", b"a:1:{i:0;", None).unwrap();
    assert_eq!(c.get(&s("k"), &s("g"), false).unwrap(), None);
    assert_eq!(store.get("g", "k").unwrap(), None);
    // `add` is no longer blocked by the bad row.
    let (next, _) = cache_over(store.clone());
    assert!(next.add(&s("k"), Value::Int(1), &s("g"), 0).unwrap());
}
