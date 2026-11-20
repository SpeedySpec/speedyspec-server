use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use speedyspec_cache::{CacheError, CachingStore};
use speedyspec_cache_memory::MemoryCache;
use speedyspec_hooks::Value;
use speedyspec_wp_object_cache::*;

fn s(v: &str) -> Value {
    Value::Str(v.into())
}

#[test]
fn persistent_iff_second_level() {
    let bare = ObjectCache::new(ObjectCachePorts::rust(), None);
    assert!(!bare.is_persistent());
    let cache = bare.with_persistent(Rc::new(MemoryCache::new()));
    assert!(cache.is_persistent());
}

#[test]
fn second_level_shared_between_instances() {
    let store: Rc<dyn CachingStore> = Rc::new(MemoryCache::new());
    let a = ObjectCache::new(ObjectCachePorts::rust(), Some(store.clone()));
    let b = ObjectCache::new(ObjectCachePorts::rust(), Some(store));
    assert!(a.set(&s("k"), s("v"), &s("g"), 0).unwrap());
    assert_eq!(b.get(&s("k"), &s("g"), false).unwrap(), Some(s("v")));
}

#[test]
fn non_persistent_group_stays_local() {
    let store: Rc<dyn CachingStore> = Rc::new(MemoryCache::new());
    let a = ObjectCache::new(ObjectCachePorts::rust(), Some(store.clone()));
    let b = ObjectCache::new(ObjectCachePorts::rust(), Some(store));
    a.add_non_persistent_groups(&[s("np")]);
    assert!(a.set(&s("k"), s("v"), &s("np"), 0).unwrap());
    assert_eq!(a.get(&s("k"), &s("np"), false).unwrap(), Some(s("v")));
    assert_eq!(b.get(&s("k"), &s("np"), false).unwrap(), None);
}

/// Every operation fails, as a stopped Valkey does.
struct Down;
fn down<T>() -> Result<T, CacheError> {
    Err(CacheError::Backend {
        cache: "down".into(),
        message: "connection refused".into(),
    })
}
impl CachingStore for Down {
    fn name(&self) -> &str {
        "down"
    }
    fn get(&self, _: &str, _: &str) -> Result<Option<Vec<u8>>, CacheError> {
        down()
    }
    fn get_many(&self, _: &str, _: &[&str]) -> Result<Vec<Option<Vec<u8>>>, CacheError> {
        down()
    }
    fn set(&self, _: &str, _: &str, _: &[u8], _: Option<Duration>) -> Result<(), CacheError> {
        down()
    }
    fn add(&self, _: &str, _: &str, _: &[u8], _: Option<Duration>) -> Result<bool, CacheError> {
        down()
    }
    fn replace(&self, _: &str, _: &str, _: &[u8], _: Option<Duration>) -> Result<bool, CacheError> {
        down()
    }
    fn delete(&self, _: &str, _: &str) -> Result<bool, CacheError> {
        down()
    }
    fn increment(&self, _: &str, _: &str, _: i64) -> Result<Option<i64>, CacheError> {
        down()
    }
    fn decrement(&self, _: &str, _: &str, _: i64) -> Result<Option<i64>, CacheError> {
        down()
    }
    fn flush(&self) -> Result<(), CacheError> {
        down()
    }
    fn flush_group(&self, _: &str) -> Result<(), CacheError> {
        down()
    }
}

#[derive(Default)]
struct Misses(RefCell<Vec<String>>);
impl CacheNotices for Misses {
    fn doing_it_wrong(&self, _: &str, _: &str, _: &str) {}
    fn deprecated_function(&self, _: &str, _: &str, _: &str) {}
    fn persistence_miss(&self, _: &str, _: &str, why: &str) {
        self.0.borrow_mut().push(why.into());
    }
}

#[test]
fn failing_second_level_logged_once_per_kind() {
    let misses = Rc::new(Misses::default());
    let mut ports = ObjectCachePorts::rust();
    ports.notices = misses.clone();
    let cache = ObjectCache::new(ports, Some(Rc::new(Down)));
    for i in 0..5 {
        let k = s(&format!("k{i}"));
        assert_eq!(cache.get(&k, &s("g"), false).unwrap(), None);
        assert!(cache.set(&k, Value::Int(i), &s("g"), 0).unwrap());
        assert_eq!(cache.get(&k, &s("g"), false).unwrap(), Some(Value::Int(i)));
    }
    // One read failure and one write failure, not one per call.
    assert_eq!(misses.0.borrow().len(), 2, "{:?}", misses.0.borrow());
}
