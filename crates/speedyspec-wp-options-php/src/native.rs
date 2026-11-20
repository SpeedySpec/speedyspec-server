//! The native functions `options.php` calls, one per WordPress function, each through a handler resolved once.

use std::rc::Rc;

use speedyspec_container::Container;
use speedyspec_cqrs::{BusError, Command, HandlerRef, Query, QueryRef};
use speedyspec_php::ext_php_rs::builders::FunctionBuilder;
use speedyspec_php::native::{self, NativeError};
use speedyspec_php_ext::{Key, Value};
use speedyspec_wp_options::{
    AddOption, AutoloadValuesToAutoload, DeleteOption, DetermineOptionAutoloadValue,
    FilterDefaultAutoloadValueViaOptionSize, GetOption, LoadAlloptions, OptionsError,
    PrimeOptionCaches, PrimeOptionCachesByGroup, ProtectSpecialOption, SetOptionAutoload,
    SetOptionAutoloadValues, SetOptionsAutoload, UpdateOption, php_string,
};

use crate::adapters::Snapshot;

const NATIVE: &str = "SpeedySpec\\WpOptions\\Native\\";

/// The handlers every native function dispatches through, each resolved once.
pub(crate) struct Handlers {
    get_option: QueryRef<GetOption>,
    load_alloptions: QueryRef<LoadAlloptions>,
    determine: QueryRef<DetermineOptionAutoloadValue>,
    size: QueryRef<FilterDefaultAutoloadValueViaOptionSize>,
    values: QueryRef<AutoloadValuesToAutoload>,
    add: HandlerRef<AddOption>,
    update: HandlerRef<UpdateOption>,
    delete: HandlerRef<DeleteOption>,
    prime: HandlerRef<PrimeOptionCaches>,
    prime_group: HandlerRef<PrimeOptionCachesByGroup>,
    set_values: HandlerRef<SetOptionAutoloadValues>,
    set_many: HandlerRef<SetOptionsAutoload>,
    set_one: HandlerRef<SetOptionAutoload>,
    protect: HandlerRef<ProtectSpecialOption>,
}

impl Handlers {
    pub(crate) fn new(c: &Rc<Container>) -> Handlers {
        Handlers {
            get_option: QueryRef::new(c.clone()),
            load_alloptions: QueryRef::new(c.clone()),
            determine: QueryRef::new(c.clone()),
            size: QueryRef::new(c.clone()),
            values: QueryRef::new(c.clone()),
            add: HandlerRef::new(c.clone()),
            update: HandlerRef::new(c.clone()),
            delete: HandlerRef::new(c.clone()),
            prime: HandlerRef::new(c.clone()),
            prime_group: HandlerRef::new(c.clone()),
            set_values: HandlerRef::new(c.clone()),
            set_many: HandlerRef::new(c.clone()),
            set_one: HandlerRef::new(c.clone()),
            protect: HandlerRef::new(c.clone()),
        }
    }
}

type R = Result<Value, NativeError>;

/// A native function's body.
type Native = fn(&[Value]) -> R;

/// An options error as PHP sees it: a PHP exception rethrown, PHP unwinding left alone, anything else an `Error`.
fn native_error(e: OptionsError) -> NativeError {
    match e {
        OptionsError::Php(Value::Null) => NativeError::Silent,
        OptionsError::Php(exception) => NativeError::Rethrow(exception),
        OptionsError::Hook(hook) => match hook
            .payload
            .as_ref()
            .and_then(|p| p.downcast_ref::<Value>())
        {
            Some(exception) => NativeError::Rethrow(exception.clone()),
            None if hook.payload.is_some() => NativeError::Silent,
            None => NativeError::throw("Error", hook.message),
        },
        other => NativeError::throw("Error", other.to_string()),
    }
}

fn bus(e: BusError) -> NativeError {
    NativeError::throw("Error", e.to_string())
}

fn ask<Q: Query<Result = Result<T, OptionsError>>, T>(
    r: &QueryRef<Q>,
    q: Q,
) -> Result<T, NativeError> {
    r.call(q).map_err(bus)?.map_err(native_error)
}

fn send<C: Command<Result = Result<T, OptionsError>>, T>(
    r: &HandlerRef<C>,
    c: C,
) -> Result<T, NativeError> {
    r.call(c).map_err(bus)?.map_err(native_error)
}

fn a(args: &[Value], i: usize) -> Value {
    args.get(i).cloned().unwrap_or(Value::Null)
}

fn or(args: &[Value], i: usize, default: Value) -> Value {
    args.get(i).cloned().unwrap_or(default)
}

/// An option name as `trim()` takes it: a scalar by PHP's string conversion; `None` for an array or object.
fn name(v: &Value) -> Option<String> {
    match v {
        Value::Array(_) | Value::Opaque(_) => None,
        other => php_string(other).map(|b| String::from_utf8_lossy(&b).into_owned()),
    }
}

/// The name `get_option`, `add_option` and `update_option` take: an array or object reaches WordPress's
/// `isset( $deprecated_keys[ $option ] )`, which throws PHP's `TypeError`; an empty array is `empty()`, so false.
fn checked_name(v: &Value) -> Result<Option<String>, NativeError> {
    match v {
        Value::Array(items) if items.is_empty() => Ok(None),
        Value::Array(_) | Value::Opaque(_) => {
            let kind = match speedyspec_php::call(
                &Value::Str("get_debug_type".into()),
                std::slice::from_ref(v),
            ) {
                Ok(Value::Str(kind)) => kind,
                _ => "array".into(),
            };
            Err(NativeError::throw(
                "TypeError",
                format!("Cannot access offset of type {kind} in isset or empty"),
            ))
        }
        other => Ok(name(other)),
    }
}

fn text(k: &Key) -> String {
    match k {
        Key::Int(i) => i.to_string(),
        Key::Str(s) => s.clone(),
    }
}

fn names(v: &Value) -> Vec<String> {
    match v.read().as_ref() {
        Value::Array(items) => items.iter().filter_map(|(_, v)| name(v)).collect(),
        _ => Vec::new(),
    }
}

/// A PHP array of name → value, keyed as PHP keys them.
fn by_name(pairs: Vec<(String, Value)>) -> Value {
    Value::Array(pairs.into_iter().map(|(n, v)| (key(&n), v)).collect())
}

fn key(name: &str) -> Key {
    let digits = name.strip_prefix('-').unwrap_or(name);
    let canonical = !digits.is_empty()
        && digits.bytes().all(|b| b.is_ascii_digit())
        && !(digits.len() > 1 && digits.starts_with('0'))
        && name != "-0";
    match canonical.then(|| name.parse::<i64>().ok()).flatten() {
        Some(i) => Key::Int(i),
        None => Key::Str(name.to_string()),
    }
}

fn h<T>(f: impl FnOnce(&Handlers) -> T) -> Result<T, NativeError> {
    crate::SHARED
        .with(|s| s.borrow().as_ref().map(|s| f(&s.handlers)))
        .ok_or_else(|| NativeError::throw("Error", "the options API is not available"))
}

fn snapshot() -> Option<Rc<Snapshot>> {
    crate::SHARED.with(|s| s.borrow().as_ref().map(|s| s.snapshot.clone()))
}

fn get_option(args: &[Value]) -> R {
    let Some(n) = checked_name(&a(args, 0))? else {
        return Ok(Value::Bool(false));
    };
    let default = args.get(1).cloned();
    h(|h| ask(&h.get_option, GetOption { name: n, default }))?
}

fn add_option(args: &[Value]) -> R {
    let Some(n) = checked_name(&a(args, 0))? else {
        return Ok(Value::Bool(false));
    };
    let m = AddOption {
        name: n,
        value: or(args, 1, Value::Str(String::new())),
        deprecated: or(args, 2, Value::Str(String::new())),
        autoload: a(args, 3),
    };
    h(|h| send(&h.add, m))?.map(Value::Bool)
}

fn update_option(args: &[Value]) -> R {
    let Some(n) = checked_name(&a(args, 0))? else {
        return Ok(Value::Bool(false));
    };
    let m = UpdateOption {
        name: n,
        value: a(args, 1),
        autoload: a(args, 2),
    };
    h(|h| send(&h.update, m))?.map(Value::Bool)
}

fn delete_option(args: &[Value]) -> R {
    let Some(n) = name(&a(args, 0)) else {
        return Ok(Value::Bool(false));
    };
    h(|h| send(&h.delete, DeleteOption { name: n }))?.map(Value::Bool)
}

fn load_alloptions(args: &[Value]) -> R {
    let force_cache = !speedyspec_wp_options::php_empty(&a(args, 0).read());
    h(|h| ask(&h.load_alloptions, LoadAlloptions { force_cache }))?
}

fn prime(args: &[Value]) -> R {
    let names = names(&a(args, 0));
    h(|h| send(&h.prime, PrimeOptionCaches { names }))??;
    Ok(Value::Null)
}

fn prime_group(args: &[Value]) -> R {
    let group = name(&a(args, 0)).unwrap_or_default();
    h(|h| send(&h.prime_group, PrimeOptionCachesByGroup { group }))??;
    Ok(Value::Null)
}

fn results(pairs: Vec<(String, bool)>) -> Value {
    by_name(
        pairs
            .into_iter()
            .map(|(n, b)| (n, Value::Bool(b)))
            .collect(),
    )
}

fn set_values(args: &[Value]) -> R {
    let options = match a(args, 0).read().as_ref() {
        Value::Array(items) => items.iter().map(|(k, v)| (text(k), v.clone())).collect(),
        _ => Vec::new(),
    };
    Ok(results(h(|h| {
        send(&h.set_values, SetOptionAutoloadValues { options })
    })??))
}

fn set_many(args: &[Value]) -> R {
    let names = names(&a(args, 0));
    let autoload = a(args, 1);
    Ok(results(h(|h| {
        send(&h.set_many, SetOptionsAutoload { names, autoload })
    })??))
}

fn set_one(args: &[Value]) -> R {
    let n = name(&a(args, 0)).unwrap_or_default();
    let autoload = a(args, 1);
    h(|h| send(&h.set_one, SetOptionAutoload { name: n, autoload }))?.map(Value::Bool)
}

fn determine(args: &[Value]) -> R {
    let m = DetermineOptionAutoloadValue {
        name: name(&a(args, 0)).unwrap_or_default(),
        value: a(args, 1),
        serialized: a(args, 2),
        autoload: a(args, 3),
    };
    h(|h| ask(&h.determine, m))?
}

fn size(args: &[Value]) -> R {
    let m = FilterDefaultAutoloadValueViaOptionSize {
        autoload: a(args, 0),
        name: name(&a(args, 1)).unwrap_or_default(),
        value: a(args, 2),
        serialized: a(args, 3),
    };
    h(|h| ask(&h.size, m))?
}

fn values(_: &[Value]) -> R {
    h(|h| ask(&h.values, AutoloadValuesToAutoload {}))?
}

fn protect(args: &[Value]) -> R {
    let n = name(&a(args, 0)).unwrap_or_default();
    h(|h| send(&h.protect, ProtectSpecialOption { name: n }))??;
    Ok(Value::Null)
}

pub(crate) fn functions() -> Vec<FunctionBuilder<'static>> {
    let entries: Vec<(&str, Native)> = vec![
        ("get_option", get_option),
        ("add_option", add_option),
        ("update_option", update_option),
        ("delete_option", delete_option),
        ("wp_load_alloptions", load_alloptions),
        ("wp_prime_option_caches", prime),
        ("wp_prime_option_caches_by_group", prime_group),
        ("wp_set_option_autoload_values", set_values),
        ("wp_set_options_autoload", set_many),
        ("wp_set_option_autoload", set_one),
        ("wp_determine_option_autoload_value", determine),
        ("wp_filter_default_autoload_value_via_option_size", size),
        ("wp_autoload_values_to_autoload", values),
        ("wp_protect_special_option", protect),
    ];
    entries
        .into_iter()
        .map(|(name, f)| {
            native::function(&format!("{NATIVE}{name}"), move |args: &[Value]| {
                // PHP is running: what the adapters read before may have changed.
                if let Some(snapshot) = snapshot() {
                    snapshot.clear();
                }
                f(args)
            })
        })
        .collect()
}
