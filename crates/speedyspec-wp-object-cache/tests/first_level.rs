use std::cell::{Cell, RefCell};
use std::rc::Rc;

use speedyspec_hooks::{Key, Value};
use speedyspec_wp_object_cache::*;

#[derive(Default)]
struct Notices {
    wrong: RefCell<Vec<(String, String, String)>>,
    deprecated: RefCell<Vec<(String, String, String)>>,
}
impl CacheNotices for Notices {
    fn doing_it_wrong(&self, function: &str, message: &str, version: &str) {
        self.wrong
            .borrow_mut()
            .push((function.into(), message.into(), version.into()));
    }
    fn deprecated_function(&self, function: &str, version: &str, replacement: &str) {
        self.deprecated
            .borrow_mut()
            .push((function.into(), version.into(), replacement.into()));
    }
    fn persistence_miss(&self, _: &str, _: &str, _: &str) {}
}

struct Site {
    multisite: bool,
    blog: i64,
}
impl SiteContext for Site {
    fn is_multisite(&self) -> bool {
        self.multisite
    }
    fn current_blog_id(&self) -> i64 {
        self.blog
    }
}

struct Suspended(Cell<bool>);
impl AdditionSuspension for Suspended {
    fn suspended(&self) -> bool {
        self.0.get()
    }
}

struct Rig {
    cache: ObjectCache,
    notices: Rc<Notices>,
    suspended: Rc<Suspended>,
}

fn rig_with(multisite: bool) -> Rig {
    let notices = Rc::new(Notices::default());
    let suspended = Rc::new(Suspended(Cell::new(false)));
    let mut ports = ObjectCachePorts::rust();
    ports.notices = notices.clone();
    ports.site = Rc::new(Site { multisite, blog: 1 });
    ports.suspension = suspended.clone();
    Rig {
        cache: ObjectCache::new(ports, None),
        notices,
        suspended,
    }
}

fn rig() -> Rig {
    rig_with(false)
}

fn s(v: &str) -> Value {
    Value::Str(v.into())
}

fn d() -> Value {
    s("default")
}

#[test]
fn add_get_set_replace_delete() {
    let c = rig().cache;
    assert_eq!(c.get(&s("k"), &d(), false).unwrap(), None);
    assert!(c.add(&s("k"), s("v"), &d(), 0).unwrap());
    assert!(!c.add(&s("k"), s("w"), &d(), 0).unwrap());
    assert_eq!(c.get(&s("k"), &d(), false).unwrap(), Some(s("v")));
    assert!(!c.replace(&s("nope"), s("x"), &d(), 0).unwrap());
    assert!(c.replace(&s("k"), s("r"), &d(), 0).unwrap());
    assert!(c.set(&s("k"), s("set"), &d(), 0).unwrap());
    assert_eq!(c.get(&s("k"), &d(), false).unwrap(), Some(s("set")));
    assert!(c.delete(&s("k"), &d()).unwrap());
    assert!(!c.delete(&s("k"), &d()).unwrap());
}

#[test]
fn stores_false_null_zero() {
    let c = rig().cache;
    for (k, v) in [
        ("f", Value::Bool(false)),
        ("n", Value::Null),
        ("z", Value::Int(0)),
    ] {
        c.set(&s(k), v.clone(), &d(), 0).unwrap();
        assert_eq!(c.get(&s(k), &d(), false).unwrap(), Some(v));
    }
}

#[test]
fn numeric_string_key_is_int_key() {
    let c = rig().cache;
    c.set(&s("5"), s("five"), &d(), 0).unwrap();
    assert_eq!(c.get(&Value::Int(5), &d(), false).unwrap(), Some(s("five")));
    for other in ["05", " 5", "-0"] {
        assert_eq!(c.get(&s(other), &d(), false).unwrap(), None, "{other:?}");
    }
    assert_eq!(array_key(&s("-5")).unwrap(), Key::Int(-5));
    assert_eq!(array_key(&Value::Bool(true)).unwrap(), Key::Int(1));
    assert_eq!(array_key(&Value::Null).unwrap(), Key::Str(String::new()));
    assert_eq!(array_key(&Value::Float(2.9)).unwrap(), Key::Int(2));
    assert!(
        matches!(array_key(&Value::Array(vec![])), Err(ObjectCacheError::IllegalOffset(t)) if t == "array")
    );
}

#[test]
fn invalid_keys() {
    let r = rig();
    for key in [
        s(""),
        s(" "),
        Value::Null,
        Value::Float(1.5),
        Value::Bool(true),
        Value::Array(vec![]),
    ] {
        assert!(!r.cache.add(&key, s("v"), &d(), 0).unwrap(), "{key:?}");
    }
    let wrong = r.notices.wrong.borrow();
    assert_eq!(wrong.len(), 6);
    assert!(
        wrong
            .iter()
            .all(|(f, _, v)| f == "WP_Object_Cache::add" && v == "6.1.0")
    );
    let messages: Vec<&str> = wrong.iter().map(|(_, m, _)| m.as_str()).collect();
    assert_eq!(
        messages,
        [
            "Cache key must not be an empty string.",
            "Cache key must not be an empty string.",
            "Cache key must be an integer or a non-empty string, NULL given.",
            "Cache key must be an integer or a non-empty string, double given.",
            "Cache key must be an integer or a non-empty string, boolean given.",
            "Cache key must be an integer or a non-empty string, array given.",
        ]
    );
    drop(wrong);
    r.cache.get(&Value::Null, &d(), false).unwrap();
    assert_eq!(
        r.notices.wrong.borrow().last().unwrap().0,
        "WP_Object_Cache::get"
    );
}

#[test]
fn empty_group_is_default() {
    let c = rig().cache;
    c.set(&s("k"), s("v"), &Value::Null, 0).unwrap();
    for group in [
        s(""),
        s("0"),
        Value::Bool(false),
        Value::Int(0),
        Value::Float(0.0),
        Value::Array(vec![]),
        d(),
    ] {
        assert_eq!(
            c.get(&s("k"), &group, false).unwrap(),
            Some(s("v")),
            "{group:?}"
        );
    }
}

#[test]
fn incr_decr() {
    let c = rig().cache;
    let g = d();
    c.set(&s("n"), Value::Int(5), &g, 0).unwrap();
    assert_eq!(c.incr(&s("n"), 1, &g).unwrap(), Some(Value::Int(6)));
    c.set(&s("x"), s("x"), &g, 0).unwrap();
    assert_eq!(c.incr(&s("x"), 1, &g).unwrap(), Some(Value::Int(1)));
    c.set(&s("two"), Value::Int(2), &g, 0).unwrap();
    assert_eq!(c.decr(&s("two"), 5, &g).unwrap(), Some(Value::Int(0)));
    c.set(&s("f"), s("1.5"), &g, 0).unwrap();
    assert_eq!(c.incr(&s("f"), 1, &g).unwrap(), Some(Value::Float(2.5)));
    c.set(&s("max"), Value::Int(i64::MAX), &g, 0).unwrap();
    assert_eq!(
        c.incr(&s("max"), 1, &g).unwrap(),
        Some(Value::Float(9.223_372_036_854_776e18))
    );
    assert_eq!(c.incr(&s("absent"), 1, &g).unwrap(), None);
    c.set(&s("str"), s(" 7 "), &g, 0).unwrap();
    assert_eq!(c.incr(&s("str"), 1, &g).unwrap(), Some(Value::Int(8)));
    assert_eq!(c.get(&s("str"), &g, false).unwrap(), Some(Value::Int(8)));
}

#[test]
fn multiple_ops() {
    let c = rig().cache;
    let g = d();
    let added = c
        .add_multiple(
            vec![
                (Key::Str("a".into()), s("1")),
                (Key::Str("b".into()), s("2")),
            ],
            &g,
            0,
        )
        .unwrap();
    assert_eq!(
        added,
        vec![(Key::Str("a".into()), true), (Key::Str("b".into()), true)]
    );
    let set = c
        .set_multiple(
            vec![(Key::Str("b".into()), s("3")), (Key::Int(7), s("7"))],
            &g,
            0,
        )
        .unwrap();
    assert_eq!(set, vec![(Key::Str("b".into()), true), (Key::Int(7), true)]);
    let got = c
        .get_multiple(&[s("a"), s("missing"), Value::Int(7), s("b")], &g, false)
        .unwrap();
    assert_eq!(
        got,
        vec![
            (Key::Str("a".into()), s("1")),
            (Key::Str("missing".into()), Value::Bool(false)),
            (Key::Int(7), s("7")),
            (Key::Str("b".into()), s("3")),
        ]
    );
    let deleted = c.delete_multiple(&[s("a"), s("zz")], &g).unwrap();
    assert_eq!(
        deleted,
        vec![(Key::Str("a".into()), true), (Key::Str("zz".into()), false)]
    );
}

#[test]
fn flush_and_flush_group() {
    let c = rig().cache;
    c.set(&s("k"), s("v"), &d(), 0).unwrap();
    c.set(&s("k"), s("v"), &s("other"), 0).unwrap();
    assert!(c.flush_group(&s("")).unwrap());
    assert!(
        c.get(&s("k"), &d(), false).unwrap().is_some(),
        "flush_group('') leaves default"
    );
    assert!(c.flush_group(&s("other")).unwrap());
    assert!(c.get(&s("k"), &s("other"), false).unwrap().is_none());
    assert!(c.flush());
    assert!(c.get(&s("k"), &d(), false).unwrap().is_none());
    assert!(c.flush_runtime());
}

#[test]
fn multisite_prefix() {
    let c = rig_with(true).cache;
    assert_eq!(c.blog_prefix(), "1:");
    c.add_global_groups(&[s("users")]);
    c.set(&s("k"), s("posts-1"), &s("posts"), 0).unwrap();
    c.set(&s("u"), s("user"), &s("users"), 0).unwrap();
    c.switch_to_blog(2);
    assert_eq!(c.get(&s("k"), &s("posts"), false).unwrap(), None);
    assert_eq!(c.get(&s("u"), &s("users"), false).unwrap(), Some(s("user")));
    assert_eq!(c.global_groups(), vec![Key::Str("users".into())]);
    let entries = c.runtime_entries();
    assert_eq!(entries[0].1[0].0, Key::Str("1:k".into()));
}

#[test]
fn single_site_ignores_switch_to_blog() {
    let c = rig().cache;
    c.set(&s("k"), s("v"), &d(), 0).unwrap();
    c.switch_to_blog(5);
    assert_eq!(c.blog_prefix(), "");
    assert_eq!(c.get(&s("k"), &d(), false).unwrap(), Some(s("v")));
}

#[test]
fn reset_keeps_global_groups_and_notes_deprecation() {
    let r = rig();
    r.cache.add_global_groups(&[s("users")]);
    r.cache.set(&s("k"), s("v"), &d(), 0).unwrap();
    r.cache.set(&s("u"), s("v"), &s("users"), 0).unwrap();
    r.cache.reset();
    assert!(r.cache.get(&s("k"), &d(), false).unwrap().is_none());
    assert!(r.cache.get(&s("u"), &s("users"), false).unwrap().is_some());
    assert_eq!(
        *r.notices.deprecated.borrow(),
        vec![(
            "reset".to_string(),
            "3.5.0".to_string(),
            "WP_Object_Cache::switch_to_blog()".to_string()
        )]
    );
}

#[test]
fn suspended_addition_refuses_add() {
    let r = rig();
    r.suspended.0.set(true);
    assert!(!r.cache.add(&s("k"), s("v"), &d(), 0).unwrap());
    assert!(
        r.notices.wrong.borrow().is_empty(),
        "suspension is checked before the key"
    );
    assert!(r.cache.set(&s("k"), s("v"), &d(), 0).unwrap());
}

#[test]
fn hit_and_miss_counts() {
    let c = rig().cache;
    c.set(&s("k"), s("v"), &d(), 0).unwrap();
    c.get(&s("k"), &d(), false).unwrap();
    c.get(&s("k"), &d(), false).unwrap();
    c.get(&s("nope"), &d(), false).unwrap();
    assert_eq!((c.hits(), c.misses()), (2, 1));
    c.set_hits(10);
    assert_eq!(c.hits(), 10);
}

#[test]
fn supports_exact_list() {
    for f in [
        "add_multiple",
        "set_multiple",
        "get_multiple",
        "delete_multiple",
        "flush_runtime",
        "flush_group",
    ] {
        assert!(ObjectCache::supports(f), "{f}");
    }
    assert!(!ObjectCache::supports("ADD_MULTIPLE"));
    assert!(!ObjectCache::supports("incr"));
}
