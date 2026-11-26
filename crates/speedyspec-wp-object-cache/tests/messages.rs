use std::rc::Rc;

use speedyspec_container::Container;
use speedyspec_cqrs::{CommandBus, CommandHandler, QueryBus, bind_command_handler};
use speedyspec_hooks::Value;
use speedyspec_wp_object_cache::*;
use speedyspec_wp_object_cache_contracts::ObjectCacheError;

fn s(v: &str) -> Value {
    Value::Str(v.into())
}

fn bound() -> (Rc<Container>, Rc<CurrentObjectCache>) {
    let container = Rc::new(Container::new());
    let current = Rc::new(CurrentObjectCache::new(ObjectCachePorts::rust(), None));
    register_object_cache_handlers(&container, current.clone());
    (container, current)
}

#[test]
fn bus_round_trip() {
    let (c, _) = bound();
    let set = CommandBus::new(c.clone())
        .dispatch(CacheSet {
            key: s("k"),
            data: s("v"),
            group: s("g"),
            expire: 0,
        })
        .unwrap();
    assert_eq!(set, Ok(true));
    let got = QueryBus::new(c.clone())
        .ask(CacheGet {
            key: s("k"),
            group: s("g"),
            force: false,
        })
        .unwrap();
    assert_eq!(got, Ok(Some(s("v"))));
    assert!(
        QueryBus::new(c)
            .ask(CacheSupports {
                feature: "flush_group".into()
            })
            .unwrap()
    );
}

#[test]
fn select_switches_target() {
    let (c, current) = bound();
    let first = current.current();
    first.set(&s("k"), s("first"), &s("g"), 0).unwrap();
    let second = current.create();
    current.select(second.clone());
    let got = QueryBus::new(c)
        .ask(CacheGet {
            key: s("k"),
            group: s("g"),
            force: false,
        })
        .unwrap();
    assert_eq!(got, Ok(None));
    assert_eq!(
        first.get(&s("k"), &s("g"), false).unwrap(),
        Some(s("first"))
    );
    assert!(Rc::ptr_eq(&current.current(), &second));
}

#[test]
fn end_request_drops_values() {
    let (_, current) = bound();
    let held: Rc<dyn std::any::Any> = Rc::new(42u8);
    current
        .current()
        .set(&s("k"), Value::Opaque(held.clone()), &s("g"), 0)
        .unwrap();
    assert_eq!(Rc::strong_count(&held), 2);
    current.end_request();
    assert_eq!(Rc::strong_count(&held), 1);
    assert_eq!(
        current.current().get(&s("k"), &s("g"), false).unwrap(),
        None
    );
}

struct Always;
impl CommandHandler<CacheSet> for Always {
    fn handle(&self, _: CacheSet) -> Result<bool, ObjectCacheError> {
        Ok(false)
    }
}

#[test]
fn override_by_key() {
    let (c, _) = bound();
    bind_command_handler::<CacheSet>(&c, |_, _| Ok(Rc::new(Always)));
    let r = CommandBus::new(c)
        .dispatch(CacheSet {
            key: s("k"),
            data: s("v"),
            group: s("g"),
            expire: 0,
        })
        .unwrap();
    assert_eq!(r, Ok(false));
}
