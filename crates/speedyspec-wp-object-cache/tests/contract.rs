//! `WpObjectCache` and `CurrentObjectCache` answer through `dyn ObjectCache` as their own methods do.

use std::cell::Cell;

use speedyspec_hooks::{Key, Value};
use speedyspec_wp_object_cache::{CurrentObjectCache, ObjectCachePorts, WpObjectCache};
use speedyspec_wp_object_cache_contracts::{ObjectCache, ObjectCacheError, get_with};

fn s(v: &str) -> Value {
    Value::Str(v.into())
}

fn k(v: &str) -> Key {
    Key::Str(v.into())
}

#[test]
fn every_method_reaches_wp_object_cache_through_dyn() {
    let concrete = WpObjectCache::new(ObjectCachePorts::rust(), None);
    let cache: &dyn ObjectCache = &concrete;
    let g = s("g");

    assert_eq!(cache.set(&s("a"), s("1"), &g, 0), Ok(true));
    assert_eq!(cache.get(&s("a"), &g, false), Ok(Some(s("1"))));
    assert_eq!(concrete.get(&s("a"), &g, false), Ok(Some(s("1"))));

    assert_eq!(cache.add(&s("a"), s("2"), &g, 0), Ok(false));
    assert_eq!(concrete.get(&s("a"), &g, false), Ok(Some(s("1"))));

    assert_eq!(
        cache.set_multiple(vec![(k("b"), s("2")), (k("c"), s("3"))], &g, 0),
        Ok(vec![(k("b"), true), (k("c"), true)])
    );
    assert_eq!(
        cache.get_multiple(&[s("b"), s("c"), s("x")], &g, false),
        Ok(vec![
            (k("b"), s("2")),
            (k("c"), s("3")),
            (k("x"), Value::Bool(false)),
        ])
    );

    assert_eq!(
        cache.add_multiple(vec![(k("c"), s("9")), (k("d"), s("4"))], &g, 0),
        Ok(vec![(k("c"), false), (k("d"), true)])
    );
    assert_eq!(concrete.get(&s("d"), &g, false), Ok(Some(s("4"))));

    assert_eq!(cache.peek(&s("d"), &g), Some(s("4")));
    assert_eq!(cache.peek(&s("x"), &g), None);

    let mut seen = Vec::new();
    cache
        .read_with(&s("d"), &g, false, &mut |v| seen.push(v.cloned()))
        .unwrap();
    assert_eq!(seen, vec![Some(s("4"))]);

    assert_eq!(cache.delete(&s("a"), &g), Ok(true));
    assert_eq!(concrete.get(&s("a"), &g, false), Ok(None));

    assert_eq!(
        cache.delete_multiple(&[s("b"), s("x")], &g),
        Ok(vec![(k("b"), true), (k("x"), false)])
    );
    assert_eq!(concrete.get(&s("b"), &g, false), Ok(None));

    cache.add_global_groups(&[s("site-options")]);
    assert!(concrete.global_groups().contains(&k("site-options")));

    assert!(!cache.is_persistent());
}

#[test]
fn current_forwards_to_the_selected_cache() {
    let current = CurrentObjectCache::new(ObjectCachePorts::rust(), None);
    let cache: &dyn ObjectCache = &current;
    let g = s("g");
    let first = current.current();

    assert_eq!(cache.set(&s("k"), s("v"), &g, 0), Ok(true));
    assert_eq!(cache.get(&s("k"), &g, false), Ok(Some(s("v"))));

    current.select(current.create());
    assert_eq!(cache.get(&s("k"), &g, false), Ok(None));

    current.select(first);
    assert_eq!(cache.get(&s("k"), &g, false), Ok(Some(s("v"))));

    current.end_request();
    assert_eq!(cache.get(&s("k"), &g, false), Ok(None));
}

#[test]
fn get_with_on_the_real_cache() {
    let concrete = WpObjectCache::new(ObjectCachePorts::rust(), None);
    let cache: &dyn ObjectCache = &concrete;
    let g = s("g");
    cache.set(&s("k"), s("v"), &g, 0).unwrap();
    let calls = Cell::new(0);
    let count = |v: Option<&Value>| {
        calls.set(calls.get() + 1);
        v.cloned()
    };

    assert_eq!(get_with(cache, &s("k"), &g, false, count), Ok(Some(s("v"))));
    assert_eq!(calls.get(), 1);

    assert_eq!(get_with(cache, &s("absent"), &g, false, count), Ok(None));
    assert_eq!(calls.get(), 2);

    // An array key is refused with a notice, as WordPress does: a miss, so `f` runs once with `None`.
    assert_eq!(
        get_with(cache, &Value::Array(vec![]), &g, false, count),
        Ok(None)
    );
    assert_eq!(calls.get(), 3);

    // A non-empty array group is PHP's illegal offset (an empty one is `default`): the error comes back and `f`
    // does not run.
    let array_group = Value::Array(vec![(Key::Int(0), s("g"))]);
    assert_eq!(
        get_with(cache, &s("k"), &array_group, false, count),
        Err(ObjectCacheError::IllegalOffset("array".into()))
    );
    assert_eq!(calls.get(), 3);
}
