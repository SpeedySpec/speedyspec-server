use std::rc::Rc;
use std::time::Duration;

use speedyspec_cache::{CacheError, CachingStore};
use speedyspec_cache_memory::MemoryCache;
use speedyspec_wp_hooks::{IgnoreDeprecations, WpHookApi};
use speedyspec_wp_hooks_contracts::HookValue;
use speedyspec_wp_hooks_memory::MemoryHookStore;
use speedyspec_wp_object_cache::{CurrentObjectCache, ObjectCache, ObjectCachePorts};
use speedyspec_wp_options::Option as OptionEntity;
use speedyspec_wp_options::{
    Autoload, ExternalObjectCache, ObjectCachePersistence, OptionName, OptionRepository,
    OptionsApi, OptionsDeps, SerializedValue,
};
use speedyspec_wp_options_memory::MemoryOptionRepository;

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

fn s(v: &str) -> HookValue {
    HookValue::Str(v.into())
}

#[test]
fn get_option_survives_failing_second_level() {
    let hooks = Rc::new(WpHookApi::new(
        Rc::new(MemoryHookStore::new()),
        Rc::new(IgnoreDeprecations),
    ));
    let current = Rc::new(CurrentObjectCache::new(
        ObjectCachePorts::rust(),
        Some(Rc::new(Down)),
    ));
    let repo = Rc::new(MemoryOptionRepository::new());
    repo.add(&OptionEntity {
        name: OptionName::new("ss_site_name").unwrap(),
        value: SerializedValue::new(b"My Site".to_vec()),
        autoload: Autoload::On,
    })
    .unwrap();
    let api = OptionsApi::new(OptionsDeps::rust(hooks, current, repo));
    assert_eq!(api.get_option("ss_site_name", None).unwrap(), s("My Site"));
}

#[test]
fn external_cache_reports_persistence() {
    let bare = Rc::new(ObjectCache::new(ObjectCachePorts::rust(), None));
    assert!(!ObjectCachePersistence(bare).using());
    let persistent = Rc::new(ObjectCache::new(
        ObjectCachePorts::rust(),
        Some(Rc::new(MemoryCache::new())),
    ));
    assert!(ObjectCachePersistence(persistent).using());
}
