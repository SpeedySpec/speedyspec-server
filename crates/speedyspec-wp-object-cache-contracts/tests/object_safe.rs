//! The object cache port is object safe: a seam holds it as `Rc<dyn ObjectCache>`.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use speedyspec_hooks::{Key, Value};
use speedyspec_wp_object_cache_contracts::{ObjectCache, ObjectCacheError, get_with};

/// A map from `(group, key)` to value; keys and groups must be strings.
#[derive(Default)]
struct MapCache {
    entries: RefCell<HashMap<(String, String), Value>>,
}

fn text(v: &Value) -> String {
    match v {
        Value::Str(s) => s.clone(),
        Value::Int(i) => i.to_string(),
        other => format!("{other:?}"),
    }
}

impl ObjectCache for MapCache {
    fn get(
        &self,
        key: &Value,
        group: &Value,
        _force: bool,
    ) -> Result<Option<Value>, ObjectCacheError> {
        Ok(self
            .entries
            .borrow()
            .get(&(text(group), text(key)))
            .cloned())
    }

    fn read_with(
        &self,
        key: &Value,
        group: &Value,
        _force: bool,
        reader: &mut dyn FnMut(Option<&Value>),
    ) -> Result<(), ObjectCacheError> {
        reader(self.entries.borrow().get(&(text(group), text(key))));
        Ok(())
    }

    fn get_multiple(
        &self,
        _keys: &[Value],
        _group: &Value,
        _force: bool,
    ) -> Result<Vec<(Key, Value)>, ObjectCacheError> {
        Ok(Vec::new())
    }

    fn peek(&self, key: &Value, group: &Value) -> Option<Value> {
        self.entries
            .borrow()
            .get(&(text(group), text(key)))
            .cloned()
    }

    fn add(
        &self,
        _key: &Value,
        _data: Value,
        _group: &Value,
        _expire: i64,
    ) -> Result<bool, ObjectCacheError> {
        Ok(false)
    }

    fn add_multiple(
        &self,
        _data: Vec<(Key, Value)>,
        _group: &Value,
        _expire: i64,
    ) -> Result<Vec<(Key, bool)>, ObjectCacheError> {
        Ok(Vec::new())
    }

    fn set(
        &self,
        key: &Value,
        data: Value,
        group: &Value,
        _expire: i64,
    ) -> Result<bool, ObjectCacheError> {
        self.entries
            .borrow_mut()
            .insert((text(group), text(key)), data);
        Ok(true)
    }

    fn set_multiple(
        &self,
        _data: Vec<(Key, Value)>,
        _group: &Value,
        _expire: i64,
    ) -> Result<Vec<(Key, bool)>, ObjectCacheError> {
        Ok(Vec::new())
    }

    fn delete(&self, _key: &Value, _group: &Value) -> Result<bool, ObjectCacheError> {
        Ok(false)
    }

    fn delete_multiple(
        &self,
        _keys: &[Value],
        _group: &Value,
    ) -> Result<Vec<(Key, bool)>, ObjectCacheError> {
        Ok(Vec::new())
    }

    fn add_global_groups(&self, _groups: &[Value]) {}

    fn is_persistent(&self) -> bool {
        false
    }
}

fn s(v: &str) -> Value {
    Value::Str(v.into())
}

#[test]
fn object_cache_is_object_safe() {
    let cache: Rc<dyn ObjectCache> = Rc::new(MapCache::default());
    assert_eq!(cache.set(&s("k"), s("v"), &s("g"), 0), Ok(true));
    assert_eq!(cache.get(&s("k"), &s("g"), false), Ok(Some(s("v"))));
}

#[test]
fn get_with_runs_once() {
    let cache: Rc<dyn ObjectCache> = Rc::new(MapCache::default());
    cache.set(&s("k"), s("v"), &s("g"), 0).unwrap();
    let calls = Cell::new(0);

    let hit = get_with(&*cache, &s("k"), &s("g"), false, |v| {
        calls.set(calls.get() + 1);
        v.cloned()
    });
    assert_eq!(hit, Ok(Some(s("v"))));
    assert_eq!(calls.get(), 1);

    let miss = get_with(&*cache, &s("absent"), &s("g"), false, |v| {
        calls.set(calls.get() + 1);
        v.cloned()
    });
    assert_eq!(miss, Ok(None));
    assert_eq!(calls.get(), 2);
}
