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

/// A cache whose `read_with` breaks the contract: it returns `Ok` without calling its reader.
struct SilentReader(MapCache);

impl ObjectCache for SilentReader {
    fn get(
        &self,
        key: &Value,
        group: &Value,
        force: bool,
    ) -> Result<Option<Value>, ObjectCacheError> {
        self.0.get(key, group, force)
    }

    fn read_with(
        &self,
        _key: &Value,
        _group: &Value,
        _force: bool,
        _reader: &mut dyn FnMut(Option<&Value>),
    ) -> Result<(), ObjectCacheError> {
        Ok(())
    }

    fn get_multiple(
        &self,
        keys: &[Value],
        group: &Value,
        force: bool,
    ) -> Result<Vec<(Key, Value)>, ObjectCacheError> {
        self.0.get_multiple(keys, group, force)
    }

    fn peek(&self, key: &Value, group: &Value) -> Option<Value> {
        self.0.peek(key, group)
    }

    fn add(
        &self,
        key: &Value,
        data: Value,
        group: &Value,
        expire: i64,
    ) -> Result<bool, ObjectCacheError> {
        self.0.add(key, data, group, expire)
    }

    fn add_multiple(
        &self,
        data: Vec<(Key, Value)>,
        group: &Value,
        expire: i64,
    ) -> Result<Vec<(Key, bool)>, ObjectCacheError> {
        self.0.add_multiple(data, group, expire)
    }

    fn set(
        &self,
        key: &Value,
        data: Value,
        group: &Value,
        expire: i64,
    ) -> Result<bool, ObjectCacheError> {
        self.0.set(key, data, group, expire)
    }

    fn set_multiple(
        &self,
        data: Vec<(Key, Value)>,
        group: &Value,
        expire: i64,
    ) -> Result<Vec<(Key, bool)>, ObjectCacheError> {
        self.0.set_multiple(data, group, expire)
    }

    fn delete(&self, key: &Value, group: &Value) -> Result<bool, ObjectCacheError> {
        self.0.delete(key, group)
    }

    fn delete_multiple(
        &self,
        keys: &[Value],
        group: &Value,
    ) -> Result<Vec<(Key, bool)>, ObjectCacheError> {
        self.0.delete_multiple(keys, group)
    }

    fn add_global_groups(&self, groups: &[Value]) {
        self.0.add_global_groups(groups)
    }

    fn is_persistent(&self) -> bool {
        self.0.is_persistent()
    }
}

#[test]
#[should_panic(expected = "ObjectCache::read_with must call its reader once when it succeeds")]
fn get_with_panics_when_read_with_skips_the_reader() {
    let cache = SilentReader(MapCache::default());
    let _ = get_with(&cache, &s("k"), &s("g"), false, |v| v.cloned());
}
