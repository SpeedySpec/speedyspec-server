//! WordPress's `wp_cache_*` functions and `WP_Object_Cache` in PHP, native over the Rust object cache (Facade).
//!
//! [`ObjectCacheArea`] declares, at the start of every request (from `wp-object-cache.php`), every function of
//! `cache.php` with WordPress's signatures and the class `WP_Object_Cache`. A function on a plain
//! `WP_Object_Cache` selects that object's cache as current and dispatches its use case through the handler bound
//! in the container (so PHP may override one); a subclass gets its own method called, as WordPress does. Each
//! object's methods act straight on its own Rust cache (Registry: PHP object id to cache).

mod ports;

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use speedyspec_cache::CachingStore;
use speedyspec_container::Container;
use speedyspec_cqrs::{Command, HandlerRef, Query, QueryRef};
use speedyspec_php::NativeArea;
use speedyspec_php::ext_php_rs::builders::FunctionBuilder;
use speedyspec_php::native::{self, NativeError};
use speedyspec_php_ext::{Key, Value};
use speedyspec_wp_object_cache::*;
use speedyspec_wp_object_cache_contracts::ObjectCacheError;

pub use ports::{PhpAdditionSuspension, PhpCacheNotices, PhpSerializeCodec, PhpSiteContext};

const FACADE: &str = include_str!("wp-object-cache.php");
const NATIVE: &str = "SpeedySpec\\WpObjectCache\\Native\\";

struct Handlers {
    add: HandlerRef<CacheAdd>,
    add_multiple: HandlerRef<CacheAddMultiple>,
    replace: HandlerRef<CacheReplace>,
    set: HandlerRef<CacheSet>,
    set_multiple: HandlerRef<CacheSetMultiple>,
    delete: HandlerRef<CacheDelete>,
    delete_multiple: HandlerRef<CacheDeleteMultiple>,
    incr: HandlerRef<CacheIncr>,
    decr: HandlerRef<CacheDecr>,
    flush: HandlerRef<CacheFlush>,
    flush_runtime: HandlerRef<CacheFlushRuntime>,
    flush_group: HandlerRef<CacheFlushGroup>,
    global_groups: HandlerRef<CacheAddGlobalGroups>,
    non_persistent: HandlerRef<CacheAddNonPersistentGroups>,
    switch_to_blog: HandlerRef<CacheSwitchToBlog>,
    reset: HandlerRef<CacheReset>,
    get: QueryRef<CacheGet>,
    get_multiple: QueryRef<CacheGetMultiple>,
    supports: QueryRef<CacheSupports>,
}

struct State {
    current: Rc<CurrentObjectCache>,
    handlers: Handlers,
    registry: RefCell<HashMap<u64, Rc<WpObjectCache>>>,
    found: Cell<bool>,
}

thread_local! {
    static STATE: RefCell<Option<Rc<State>>> = const { RefCell::new(None) };
}

fn state() -> Result<Rc<State>, NativeError> {
    STATE
        .with(|s| s.borrow().clone())
        .ok_or_else(|| NativeError::throw("Error", "the object cache is not available"))
}

/// The native area of WordPress's object cache.
pub struct ObjectCacheArea {
    state: Rc<State>,
}

impl ObjectCacheArea {
    /// The object cache with the PHP ports, and its handlers registered in `container`. `persistent` is the second
    /// level (none in phase 1, as WordPress's default).
    pub fn new(
        container: Rc<Container>,
        persistent: Option<Rc<dyn CachingStore>>,
    ) -> ObjectCacheArea {
        let ports = ObjectCachePorts {
            site: Rc::new(PhpSiteContext),
            notices: Rc::new(PhpCacheNotices),
            suspension: Rc::new(PhpAdditionSuspension),
            codec: Rc::new(PhpSerializeCodec),
        };
        let current = Rc::new(CurrentObjectCache::new(ports, persistent));
        register_object_cache_handlers(&container, current.clone());
        let c = &container;
        let handlers = Handlers {
            add: HandlerRef::new(c.clone()),
            add_multiple: HandlerRef::new(c.clone()),
            replace: HandlerRef::new(c.clone()),
            set: HandlerRef::new(c.clone()),
            set_multiple: HandlerRef::new(c.clone()),
            delete: HandlerRef::new(c.clone()),
            delete_multiple: HandlerRef::new(c.clone()),
            incr: HandlerRef::new(c.clone()),
            decr: HandlerRef::new(c.clone()),
            flush: HandlerRef::new(c.clone()),
            flush_runtime: HandlerRef::new(c.clone()),
            flush_group: HandlerRef::new(c.clone()),
            global_groups: HandlerRef::new(c.clone()),
            non_persistent: HandlerRef::new(c.clone()),
            switch_to_blog: HandlerRef::new(c.clone()),
            reset: HandlerRef::new(c.clone()),
            get: QueryRef::new(c.clone()),
            get_multiple: QueryRef::new(c.clone()),
            supports: QueryRef::new(c.clone()),
        };
        let state = Rc::new(State {
            current,
            handlers,
            registry: RefCell::new(HashMap::new()),
            found: Cell::new(false),
        });
        STATE.with(|s| *s.borrow_mut() = Some(state.clone()));
        ObjectCacheArea { state }
    }

    /// The object cache for Rust callers.
    pub fn api(&self) -> Rc<CurrentObjectCache> {
        self.state.current.clone()
    }
}

/// Runs the object-cache scenarios through the native functions (the port side of `compat/object-cache`).
pub fn object_cache_scenario_runner(php: speedyspec_php::Php) -> speedyspec_compat::PhpRunner {
    speedyspec_compat::PhpRunner::new(php, |php| {
        php.eval(
            "speedyspec://compat/object-cache-stubs",
            speedyspec_compat::OBJECT_CACHE_STUBS,
        )
        .and_then(|_| {
            php.eval(
                "speedyspec://compat/object-cache-init",
                "wp_cache_init(); return true;",
            )
        })
        .map(|_| ())
        .map_err(|e| e.to_string())
    })
}

// ----- conversions

fn a(args: &[Value], i: usize) -> Value {
    args.get(i).cloned().unwrap_or(Value::Null)
}

fn int(v: &Value) -> i64 {
    match v {
        Value::Int(i) => *i,
        Value::Bool(b) => i64::from(*b),
        Value::Float(f) if f.is_finite() => *f as i64,
        Value::Str(s) => {
            let t = s.trim_start();
            let end = t
                .char_indices()
                .take_while(|(i, c)| c.is_ascii_digit() || (*i == 0 && (*c == '-' || *c == '+')))
                .count();
            t[..end].parse().unwrap_or(0)
        }
        _ => 0,
    }
}

fn list(v: &Value) -> Vec<Value> {
    match v {
        Value::Array(items) => items.iter().map(|(_, v)| v.clone()).collect(),
        Value::Null => Vec::new(),
        other => vec![other.clone()],
    }
}

fn pairs(v: &Value) -> Vec<(Key, Value)> {
    match v {
        Value::Array(items) => items.clone(),
        _ => Vec::new(),
    }
}

fn err(e: ObjectCacheError) -> NativeError {
    match e {
        ObjectCacheError::IllegalOffset(_) => NativeError::throw("TypeError", e.to_string()),
        ObjectCacheError::Unsupported(_) => NativeError::throw("Error", e.to_string()),
    }
}

fn bus(e: speedyspec_cqrs::BusError) -> NativeError {
    NativeError::throw("Error", e.to_string())
}

fn bools(v: Vec<(Key, bool)>) -> Value {
    Value::Array(v.into_iter().map(|(k, b)| (k, Value::Bool(b))).collect())
}

fn keyed(v: Vec<(Key, Value)>) -> Value {
    Value::Array(
        v.into_iter()
            .map(|(k, v)| (k, ports::materialise(v)))
            .collect(),
    )
}

/// The object's Rust cache; `None` for an object the registry does not know (a cleared request).
fn cache_of(s: &State, object: &Value) -> Option<Rc<WpObjectCache>> {
    // The facade passes `spl_object_id()`: an object handle would be released through PHP on every call.
    let id = match object {
        Value::Int(i) => u64::try_from(*i).ok()?,
        other => speedyspec_php::object_id(other)?,
    };
    s.registry.borrow().get(&id).cloned()
}

/// Selects the object's cache as current (when it is not already) for a `wp_cache_*` call.
fn select(s: &State, object: &Value) {
    if let Some(cache) = cache_of(s, object)
        && !Rc::ptr_eq(&cache, &s.current.current())
    {
        s.current.select(cache);
    }
}

fn cmd<C: Command>(r: &HandlerRef<C>, m: C) -> Result<C::Result, NativeError> {
    r.call(m).map_err(bus)
}

fn ask<Q: Query>(r: &QueryRef<Q>, m: Q) -> Result<Q::Result, NativeError> {
    r.call(m).map_err(bus)
}

type R = Result<Value, NativeError>;

/// One use case: through the bus for `wp_cache_*`, straight on the object's cache for its methods.
macro_rules! natives {
    ($( $name:ident ($s:ident, $args:ident, $c:ident) { bus: $bus:expr, direct: $direct:expr } )*) => {
        $(
            #[allow(unused_variables)]
            fn $name(via_bus: bool, $args: &[Value]) -> R {
                let $s = state()?;
                if via_bus {
                    select(&$s, &a($args, 0));
                    $bus
                } else {
                    let Some($c) = cache_of(&$s, &a($args, 0)) else { return Ok(Value::Bool(false)) };
                    $direct
                }
            }
        )*
        fn table() -> Vec<(&'static str, fn(bool, &[Value]) -> R)> {
            vec![$((stringify!($name), $name as fn(bool, &[Value]) -> R)),*]
        }
    };
}

natives! {
    add(s, x, c) {
        bus: Ok(Value::Bool(cmd(&s.handlers.add, CacheAdd { key: a(x, 1), data: a(x, 2), group: a(x, 3), expire: int(&a(x, 4)) })?.map_err(err)?)),
        direct: Ok(Value::Bool(c.add(&a(x, 1), a(x, 2), &a(x, 3), int(&a(x, 4))).map_err(err)?))
    }
    add_multiple(s, x, c) {
        bus: Ok(bools(cmd(&s.handlers.add_multiple, CacheAddMultiple { data: pairs(&a(x, 1)), group: a(x, 2), expire: int(&a(x, 3)) })?.map_err(err)?)),
        direct: Ok(bools(c.add_multiple(pairs(&a(x, 1)), &a(x, 2), int(&a(x, 3))).map_err(err)?))
    }
    replace(s, x, c) {
        bus: Ok(Value::Bool(cmd(&s.handlers.replace, CacheReplace { key: a(x, 1), data: a(x, 2), group: a(x, 3), expire: int(&a(x, 4)) })?.map_err(err)?)),
        direct: Ok(Value::Bool(c.replace(&a(x, 1), a(x, 2), &a(x, 3), int(&a(x, 4))).map_err(err)?))
    }
    set(s, x, c) {
        bus: Ok(Value::Bool(cmd(&s.handlers.set, CacheSet { key: a(x, 1), data: a(x, 2), group: a(x, 3), expire: int(&a(x, 4)) })?.map_err(err)?)),
        direct: Ok(Value::Bool(c.set(&a(x, 1), a(x, 2), &a(x, 3), int(&a(x, 4))).map_err(err)?))
    }
    set_multiple(s, x, c) {
        bus: Ok(bools(cmd(&s.handlers.set_multiple, CacheSetMultiple { data: pairs(&a(x, 1)), group: a(x, 2), expire: int(&a(x, 3)) })?.map_err(err)?)),
        direct: Ok(bools(c.set_multiple(pairs(&a(x, 1)), &a(x, 2), int(&a(x, 3))).map_err(err)?))
    }
    get(s, x, c) {
        bus: {
            let v = ask(&s.handlers.get, CacheGet { key: a(x, 1), group: a(x, 2), force: matches!(a(x, 3), Value::Bool(true)) })?.map_err(err)?;
            s.found.set(v.is_some());
            Ok(v.map_or(Value::Bool(false), ports::materialise))
        },
        direct: {
            let v = c.get(&a(x, 1), &a(x, 2), matches!(a(x, 3), Value::Bool(true))).map_err(err)?;
            s.found.set(v.is_some());
            Ok(v.map_or(Value::Bool(false), ports::materialise))
        }
    }
    get_multiple(s, x, c) {
        bus: Ok(keyed(ask(&s.handlers.get_multiple, CacheGetMultiple { keys: list(&a(x, 1)), group: a(x, 2), force: matches!(a(x, 3), Value::Bool(true)) })?.map_err(err)?)),
        direct: Ok(keyed(c.get_multiple(&list(&a(x, 1)), &a(x, 2), matches!(a(x, 3), Value::Bool(true))).map_err(err)?))
    }
    delete(s, x, c) {
        bus: Ok(Value::Bool(cmd(&s.handlers.delete, CacheDelete { key: a(x, 1), group: a(x, 2) })?.map_err(err)?)),
        direct: Ok(Value::Bool(c.delete(&a(x, 1), &a(x, 2)).map_err(err)?))
    }
    delete_multiple(s, x, c) {
        bus: Ok(bools(cmd(&s.handlers.delete_multiple, CacheDeleteMultiple { keys: list(&a(x, 1)), group: a(x, 2) })?.map_err(err)?)),
        direct: Ok(bools(c.delete_multiple(&list(&a(x, 1)), &a(x, 2)).map_err(err)?))
    }
    incr(s, x, c) {
        bus: Ok(cmd(&s.handlers.incr, CacheIncr { key: a(x, 1), offset: int(&a(x, 2)), group: a(x, 3) })?.map_err(err)?.unwrap_or(Value::Bool(false))),
        direct: Ok(c.incr(&a(x, 1), int(&a(x, 2)), &a(x, 3)).map_err(err)?.unwrap_or(Value::Bool(false)))
    }
    decr(s, x, c) {
        bus: Ok(cmd(&s.handlers.decr, CacheDecr { key: a(x, 1), offset: int(&a(x, 2)), group: a(x, 3) })?.map_err(err)?.unwrap_or(Value::Bool(false))),
        direct: Ok(c.decr(&a(x, 1), int(&a(x, 2)), &a(x, 3)).map_err(err)?.unwrap_or(Value::Bool(false)))
    }
    flush(s, x, c) {
        bus: Ok(Value::Bool(cmd(&s.handlers.flush, CacheFlush {})?)),
        direct: Ok(Value::Bool(c.flush()))
    }
    flush_runtime(s, x, c) {
        bus: Ok(Value::Bool(cmd(&s.handlers.flush_runtime, CacheFlushRuntime {})?)),
        direct: Ok(Value::Bool(c.flush_runtime()))
    }
    flush_group(s, x, c) {
        bus: Ok(Value::Bool(cmd(&s.handlers.flush_group, CacheFlushGroup { group: a(x, 1) })?.map_err(err)?)),
        direct: Ok(Value::Bool(c.flush_group(&a(x, 1)).map_err(err)?))
    }
    add_global_groups(s, x, c) {
        bus: { cmd(&s.handlers.global_groups, CacheAddGlobalGroups { groups: list(&a(x, 1)) })?; Ok(Value::Null) },
        direct: { c.add_global_groups(&list(&a(x, 1))); Ok(Value::Null) }
    }
    add_non_persistent_groups(s, x, c) {
        bus: { cmd(&s.handlers.non_persistent, CacheAddNonPersistentGroups { groups: list(&a(x, 1)) })?; Ok(Value::Null) },
        direct: { c.add_non_persistent_groups(&list(&a(x, 1))); Ok(Value::Null) }
    }
    switch_to_blog(s, x, c) {
        bus: { cmd(&s.handlers.switch_to_blog, CacheSwitchToBlog { blog_id: int(&a(x, 1)) })?; Ok(Value::Null) },
        direct: { c.switch_to_blog(int(&a(x, 1))); Ok(Value::Null) }
    }
    reset(s, x, c) {
        bus: { cmd(&s.handlers.reset, CacheReset {})?; Ok(Value::Null) },
        direct: { c.reset(); Ok(Value::Null) }
    }
}

fn n_supports(args: &[Value]) -> R {
    let s = state()?;
    let feature = match a(args, 0) {
        Value::Str(f) => f,
        other => format!("{other:?}"),
    };
    Ok(Value::Bool(ask(
        &s.handlers.supports,
        CacheSupports { feature },
    )?))
}

fn n_found(_: &[Value]) -> R {
    Ok(Value::Bool(state()?.found.get()))
}

fn n_select(args: &[Value]) -> R {
    select(&*state()?, &a(args, 0));
    Ok(Value::Null)
}

fn n_construct(args: &[Value]) -> R {
    let s = state()?;
    let id = speedyspec_php::object_id(&a(args, 0))
        .ok_or_else(|| NativeError::throw("Error", "not an object"))?;
    let cache = s.current.create();
    s.registry.borrow_mut().insert(id, cache);
    Ok(Value::Null)
}

fn n_destruct(args: &[Value]) -> R {
    let s = state()?;
    if let Some(id) = speedyspec_php::object_id(&a(args, 0)) {
        s.registry.borrow_mut().remove(&id);
    }
    Ok(Value::Null)
}

fn text(k: &Key) -> String {
    match k {
        Key::Int(i) => i.to_string(),
        Key::Str(s) => s.clone(),
    }
}

/// `['ok', value]` for the properties Rust keeps, `['none']` for any other (a dynamic property).
fn n_prop_get(args: &[Value]) -> R {
    let s = state()?;
    let Some(c) = cache_of(&s, &a(args, 0)) else {
        return Ok(Value::Array(vec![(Key::Int(0), Value::Str("none".into()))]));
    };
    let name = match a(args, 1) {
        Value::Str(n) => n,
        _ => String::new(),
    };
    let value = match name.as_str() {
        "cache_hits" => Value::Int(c.hits()),
        "cache_misses" => Value::Int(c.misses()),
        "blog_prefix" => Value::Str(c.blog_prefix()),
        "multisite" => Value::Bool(c.is_multisite()),
        "global_groups" => Value::Array(
            c.global_groups()
                .into_iter()
                .map(|k| (k, Value::Bool(true)))
                .collect(),
        ),
        "cache" => Value::Array(
            c.runtime_entries()
                .into_iter()
                .map(|(g, e)| (g, keyed(e)))
                .collect(),
        ),
        _ => return Ok(Value::Array(vec![(Key::Int(0), Value::Str("none".into()))])),
    };
    Ok(Value::Array(vec![
        (Key::Int(0), Value::Str("ok".into())),
        (Key::Int(1), value),
    ]))
}

/// Writes a property Rust keeps; false for any other.
fn n_prop_set(args: &[Value]) -> R {
    let s = state()?;
    let Some(c) = cache_of(&s, &a(args, 0)) else {
        return Ok(Value::Bool(false));
    };
    let value = a(args, 2);
    let name = match a(args, 1) {
        Value::Str(n) => n,
        _ => String::new(),
    };
    match name.as_str() {
        "cache_hits" => c.set_hits(int(&value)),
        "cache_misses" => c.set_misses(int(&value)),
        "blog_prefix" => c.set_blog_prefix(match &value {
            Value::Str(s) => s.clone(),
            other => format!("{}", int(other)),
        }),
        "multisite" => c.set_multisite(!matches!(
            value,
            Value::Null | Value::Bool(false) | Value::Int(0)
        )),
        "cache" => c.set_runtime_entries(
            pairs(&value)
                .into_iter()
                .map(|(g, e)| (g, pairs(&e)))
                .collect(),
        ),
        "global_groups" => {
            let groups: Vec<Value> = pairs(&value)
                .into_iter()
                .map(|(k, _)| Value::Str(text(&k)))
                .collect();
            c.add_global_groups(&groups);
        }
        _ => return Ok(Value::Bool(false)),
    }
    Ok(Value::Bool(true))
}

impl NativeArea for ObjectCacheArea {
    fn functions(&self) -> Vec<FunctionBuilder<'static>> {
        let mut out = Vec::new();
        for (name, f) in table() {
            out.push(native::function(
                &format!("{NATIVE}cache_{name}"),
                move |args: &[Value]| f(true, args),
            ));
            out.push(native::function(
                &format!("{NATIVE}object_{name}"),
                move |args: &[Value]| f(false, args),
            ));
        }
        type N = fn(&[Value]) -> R;
        let extra: [(&str, N); 7] = [
            ("cache_supports", n_supports),
            ("found", n_found),
            ("select", n_select),
            ("object_construct", n_construct),
            ("object_destruct", n_destruct),
            ("object_prop_get", n_prop_get),
            ("object_prop_set", n_prop_set),
        ];
        out.extend(
            extra
                .into_iter()
                .map(|(name, f)| native::function(&format!("{NATIVE}{name}"), f)),
        );
        out
    }

    fn request_start(&self) {
        if let Err(err) = speedyspec_php::eval("speedyspec://wp-object-cache", FACADE) {
            eprintln!("speedyspec_wordpress: declaring the object cache failed: {err}");
        }
    }

    fn request_end(&self) {
        // Every PHP value the caches hold goes before PHP's request shutdown.
        self.state.registry.borrow_mut().clear();
        self.state.current.end_request();
    }
}
