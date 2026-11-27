//! PHP values and the post type rules' values (`PhpValue`), both ways (Adapter).
//!
//! The rules read a post type's arguments and properties as `PhpValue`s. A PHP object crosses in one of two ways:
//!
//! - a `stdClass` (the `cap` and `labels` objects, a status, an object passed as `labels` or `supports`) is copied
//!   with its properties, so the rules can read inside it;
//! - any other object (a meta box callback's closure, `[$this, 'method']`, a REST controller) stays the object
//!   itself: a marker naming a slot in this request's table of live values, so its identity survives the trip.
//!
//! [`to_php`] turns a `PhpValue` back into a PHP value: a marker is the very object it names, a copied `stdClass` is
//! cast from its properties (`(object) $properties`), anything else is built by PHP's `unserialize()`.

use std::cell::RefCell;
use std::collections::HashMap;

use speedyspec_php_ext::{Key, Value};
use speedyspec_php_serialize::semantics::is_integer_key;
use speedyspec_php_serialize::{OpaquePhpValue, PhpKey, PhpValue, serialize};

/// The class of the marker that stands for a live PHP value: no PHP class can have this name.
const LIVE_CLASS: &str = "\0SpeedySpec\\WpPosts\\Live";

/// How deep `stdClass` copies go; below this an object stays live, so a cycle cannot recurse forever.
const MAX_DEPTH: usize = 16;

/// The PHP helpers the facade declares (`post-types.php`).
pub(crate) const HELPERS: &str = "SpeedySpec\\WpPosts\\Php\\";

#[derive(Default)]
struct Live {
    values: Vec<Value>,
    by_object: HashMap<u64, usize>,
}

thread_local! {
    static LIVE: RefCell<Live> = RefCell::new(Live::default());
}

/// Forgets the live values: the request that made them is over.
pub(crate) fn clear_live() {
    LIVE.with(|l| *l.borrow_mut() = Live::default());
}

fn keep(value: &Value) -> PhpValue {
    let index = LIVE.with(|l| {
        let mut l = l.borrow_mut();
        let object = speedyspec_php::object_id(value);
        if let Some(index) = object.and_then(|id| l.by_object.get(&id).copied()) {
            return index;
        }
        l.values.push(value.clone());
        let index = l.values.len() - 1;
        if let Some(id) = object {
            l.by_object.insert(id, index);
        }
        index
    });
    PhpValue::Custom {
        class: LIVE_CLASS.to_owned(),
        data: index.to_string().into_bytes(),
    }
}

fn live(data: &[u8]) -> Option<Value> {
    let index: usize = std::str::from_utf8(data).ok()?.parse().ok()?;
    LIVE.with(|l| l.borrow().values.get(index).cloned())
}

/// A PHP failure: the exception PHP threw, or `None` when PHP is unwinding or absent.
#[derive(Debug)]
pub struct Thrown(pub Option<Value>);

/// Calls a PHP callable, keeping the exception it throws.
pub(crate) fn call(callable: &Value, args: &[Value]) -> Result<Value, Thrown> {
    speedyspec_php::call_catching(callable, args).map_err(|failure| Thrown(failure.exception))
}

/// Calls a PHP function by name, keeping the exception it throws.
pub(crate) fn php(name: &str, args: &[Value]) -> Result<Value, Thrown> {
    call(&Value::Str(name.into()), args)
}

/// Calls one of the facade's helpers.
pub(crate) fn helper(name: &str, args: &[Value]) -> Result<Value, Thrown> {
    php(&format!("{HELPERS}{name}"), args)
}

/// `$object->$method( ...$args )`.
pub(crate) fn method(object: &Value, name: &str, args: &[Value]) -> Result<Value, Thrown> {
    let callable = Value::Array(vec![
        (Key::Int(0), object.clone()),
        (Key::Int(1), Value::Str(name.into())),
    ]);
    call(&callable, args)
}

/// Whether a PHP value is an object.
pub(crate) fn is_object(value: &Value) -> bool {
    speedyspec_php::object_id(&value.read()).is_some()
}

/// `get_object_vars( $object )`, as `(name, value)` pairs in PHP's order.
pub(crate) fn object_vars(object: &Value) -> Result<Vec<(Key, Value)>, Thrown> {
    match php("get_object_vars", std::slice::from_ref(object))?
        .read()
        .into_owned()
    {
        Value::Array(items) => Ok(items),
        _ => Ok(Vec::new()),
    }
}

/// `$object->$name`'s value from its properties, `None` when it has none.
pub(crate) fn property<'a>(vars: &'a [(Key, Value)], name: &str) -> Option<&'a Value> {
    vars.iter()
        .find(|(k, _)| matches!(k, Key::Str(s) if s == name))
        .map(|(_, v)| v)
}

fn bytes_of(value: &Value) -> Option<Vec<u8>> {
    match value {
        Value::Str(s) => Some(s.as_bytes().to_vec()),
        Value::Bytes(b) => Some(b.clone()),
        _ => None,
    }
}

fn php_key(key: &Key) -> PhpKey {
    match key {
        Key::Int(i) => PhpKey::Int(*i),
        Key::Str(s) => PhpKey::Str(s.as_bytes().to_vec()),
    }
}

/// A property name: an object's properties are named by strings, `5` as `"5"`.
fn property_key(key: &Key) -> PhpKey {
    match key {
        Key::Int(i) => PhpKey::Str(i.to_string().into_bytes()),
        Key::Str(s) => PhpKey::Str(s.as_bytes().to_vec()),
    }
}

/// A PHP value as the rules take it (see the module's docs).
pub fn from_php(value: &Value) -> Result<PhpValue, Thrown> {
    convert(value, 0)
}

fn convert(value: &Value, depth: usize) -> Result<PhpValue, Thrown> {
    let read = value.read();
    Ok(match &*read {
        Value::Null => PhpValue::Null,
        Value::Bool(b) => PhpValue::Bool(*b),
        Value::Int(i) => PhpValue::Int(*i),
        Value::Float(f) => PhpValue::Float(*f),
        Value::Str(s) => PhpValue::Str(s.as_bytes().to_vec()),
        Value::Bytes(b) => PhpValue::Str(b.clone()),
        Value::Array(items) => PhpValue::Array(
            items
                .iter()
                .map(|(k, v)| Ok((php_key(k), convert(v, depth + 1)?)))
                .collect::<Result<_, Thrown>>()?,
        ),
        Value::Opaque(any) => {
            if let Some(carried) = any.downcast_ref::<OpaquePhpValue>() {
                return Ok(carried.0.clone());
            }
            if speedyspec_php::object_id(&read).is_none() || depth >= MAX_DEPTH {
                return Ok(keep(&read));
            }
            let class = php("get_class", std::slice::from_ref(&*read))?;
            if bytes_of(&class).as_deref() != Some(b"stdClass".as_slice()) {
                return Ok(keep(&read));
            }
            let properties = object_vars(&read)?
                .iter()
                .map(|(k, v)| Ok((property_key(k), convert(v, depth + 1)?)))
                .collect::<Result<_, Thrown>>()?;
            PhpValue::Object {
                class: "stdClass".to_owned(),
                properties,
            }
        }
    })
}

/// `unserialize( serialize( $value ) )` in PHP: the value as PHP builds it.
fn built_by_php(value: &PhpValue) -> Result<Value, Thrown> {
    let text = serialize(value);
    let text = match String::from_utf8(text) {
        Ok(s) => Value::Str(s),
        Err(e) => Value::Bytes(e.into_bytes()),
    };
    php("unserialize", &[text])
}

/// An array key as PHP stores it: a canonical integer string is an int key.
fn array_key(key: &PhpKey) -> Key {
    match key {
        PhpKey::Int(i) => Key::Int(*i),
        PhpKey::Str(s) => {
            if is_integer_key(s)
                && let Some(i) = std::str::from_utf8(s).ok().and_then(|s| s.parse().ok())
            {
                return Key::Int(i);
            }
            Key::Str(String::from_utf8_lossy(s).into_owned())
        }
    }
}

fn pairs(items: &[(PhpKey, PhpValue)], key: fn(&PhpKey) -> Key) -> Result<Value, Thrown> {
    Ok(Value::Array(
        items
            .iter()
            .map(|(k, v)| Ok((key(k), to_php(v)?)))
            .collect::<Result<_, Thrown>>()?,
    ))
}

/// A `PhpValue` as PHP holds it (see the module's docs). A key that is not UTF-8 is read lossily.
pub fn to_php(value: &PhpValue) -> Result<Value, Thrown> {
    Ok(match value {
        PhpValue::Null => Value::Null,
        PhpValue::Bool(b) => Value::Bool(*b),
        PhpValue::Int(i) => Value::Int(*i),
        PhpValue::Float(f) => Value::Float(*f),
        PhpValue::Str(s) => string(s),
        PhpValue::Array(items) => pairs(items, array_key)?,
        PhpValue::Object { class, properties } if class == "stdClass" => {
            let properties = pairs(properties, |k| match k {
                PhpKey::Int(i) => Key::Str(i.to_string()),
                PhpKey::Str(s) => Key::Str(String::from_utf8_lossy(s).into_owned()),
            })?;
            helper("object", &[properties])?
        }
        PhpValue::Custom { class, data } if class == LIVE_CLASS => {
            live(data).unwrap_or(Value::Null)
        }
        other => built_by_php(other)?,
    })
}

/// Bytes as a PHP string value: text when it is UTF-8.
pub fn string(bytes: &[u8]) -> Value {
    match std::str::from_utf8(bytes) {
        Ok(s) => Value::Str(s.to_owned()),
        Err(_) => Value::Bytes(bytes.to_vec()),
    }
}
