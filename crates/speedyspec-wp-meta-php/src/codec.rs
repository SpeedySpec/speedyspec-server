//! PHP values and the metadata API's values (`PhpValue`), both ways (Adapter).
//!
//! The use cases take PHP's serialized form, `PhpValue`, because they serialize meta values for the database. A PHP
//! object crosses in one of two ways:
//!
//! - [`from_php`] copies it through PHP's own `serialize()`, so a meta value holds the object as `meta.php` would
//!   store it;
//! - [`from_php_live`] keeps the object itself, as a marker naming a slot in this request's table of live values, for
//!   the values `register_meta()` only passes along (its callbacks, its `$wp_meta_keys` entry), whose identity
//!   matters (`[$this, 'method']`, a closure).
//!
//! [`to_php`] turns a `PhpValue` back into a PHP value: a marker is the very object it names, any other object is
//! built by PHP's `unserialize()`.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use speedyspec_php_ext::{Key, Value};
use speedyspec_php_serialize::{OpaquePhpValue, PhpKey, PhpValue, serialize, unserialize};

/// The class of the marker that stands for a live PHP value: no PHP class can have this name.
const LIVE_CLASS: &str = "\0SpeedySpec\\WpMeta\\Live";

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

/// A PHP failure while converting: the exception PHP threw, or `None` when PHP is unwinding or absent.
#[derive(Debug)]
pub struct Thrown(pub Option<Value>);

/// Calls a PHP function, keeping the exception it throws.
pub(crate) fn php(name: &str, args: &[Value]) -> Result<Value, Thrown> {
    speedyspec_php::call_catching(&Value::Str(name.into()), args)
        .map_err(|failure| Thrown(failure.exception))
}

fn bytes_of(value: &Value) -> Option<Vec<u8>> {
    match value {
        Value::Str(s) => Some(s.as_bytes().to_vec()),
        Value::Bytes(b) => Some(b.clone()),
        _ => None,
    }
}

/// A PHP value as the use cases take it; an object is copied through PHP's `serialize()`.
pub fn from_php(value: &Value) -> Result<PhpValue, Thrown> {
    convert(value, false)
}

/// A PHP value as the use cases take it; an object stays the object itself (see the module's docs).
pub fn from_php_live(value: &Value) -> PhpValue {
    convert(value, true).unwrap_or(PhpValue::Null)
}

fn convert(value: &Value, keep_live: bool) -> Result<PhpValue, Thrown> {
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
                .map(|(k, v)| {
                    let key = match k {
                        Key::Int(i) => PhpKey::Int(*i),
                        Key::Str(s) => PhpKey::Str(s.as_bytes().to_vec()),
                    };
                    Ok((key, convert(v, keep_live)?))
                })
                .collect::<Result<_, Thrown>>()?,
        ),
        Value::Opaque(any) => {
            if let Some(carried) = any.downcast_ref::<OpaquePhpValue>() {
                return Ok(carried.0.clone());
            }
            if keep_live || speedyspec_php::object_id(&read).is_none() {
                return Ok(keep(&read));
            }
            let text = php("serialize", std::slice::from_ref(&*read))?;
            let bytes = bytes_of(&text).ok_or(Thrown(None))?;
            unserialize(&bytes).map_err(|_| Thrown(None))?
        }
    })
}

fn has_refs(value: &PhpValue) -> bool {
    match value {
        PhpValue::Ref(_) | PhpValue::ObjectRef(_) => true,
        PhpValue::Array(items)
        | PhpValue::Object {
            properties: items, ..
        } => items.iter().any(|(_, v)| has_refs(v)),
        _ => false,
    }
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

/// A `PhpValue` as PHP holds it.
pub fn to_php(value: &PhpValue) -> Result<Value, Thrown> {
    if has_refs(value) {
        return built_by_php(value);
    }
    to_php_part(value)
}

fn to_php_part(value: &PhpValue) -> Result<Value, Thrown> {
    Ok(match value {
        PhpValue::Null => Value::Null,
        PhpValue::Bool(b) => Value::Bool(*b),
        PhpValue::Int(i) => Value::Int(*i),
        PhpValue::Float(f) => Value::Float(*f),
        PhpValue::Str(s) => string(s),
        PhpValue::Array(items) => {
            let mut out = Vec::with_capacity(items.len());
            for (k, v) in items {
                let key = match k {
                    PhpKey::Int(i) => Key::Int(*i),
                    PhpKey::Str(s) => match String::from_utf8(s.clone()) {
                        Ok(s) => Key::Str(s),
                        // A key that is not UTF-8 is PHP's to hold.
                        Err(_) => return built_by_php(value),
                    },
                };
                out.push((key, to_php_part(v)?));
            }
            Value::Array(out)
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

/// Whether a hook value carries a `PhpValue` PHP cannot read as it is (an object, a marker).
pub(crate) fn carries_php_values(value: &Value) -> bool {
    match value {
        Value::Opaque(any) => any.is::<OpaquePhpValue>(),
        Value::Array(items) => items.iter().any(|(_, v)| carries_php_values(v)),
        _ => false,
    }
}

/// A hook value with every carried `PhpValue` made a PHP value, for a PHP callback to read.
pub(crate) fn hook_in(value: Value) -> Result<Value, Thrown> {
    if !carries_php_values(&value) {
        return Ok(value);
    }
    Ok(match value {
        Value::Opaque(any) => match any.downcast_ref::<OpaquePhpValue>() {
            Some(carried) => to_php(&carried.0)?,
            None => Value::Opaque(any),
        },
        Value::Array(items) => Value::Array(
            items
                .into_iter()
                .map(|(k, v)| Ok((k, hook_in(v)?)))
                .collect::<Result<_, Thrown>>()?,
        ),
        other => other,
    })
}

/// A hook's answer with every live PHP object copied into a carried `PhpValue`, so the use cases can read it.
pub(crate) fn hook_out(value: Value) -> Result<Value, Thrown> {
    let read = value.read();
    Ok(match &*read {
        Value::Array(items) => Value::Array(
            items
                .iter()
                .map(|(k, v)| Ok((k.clone(), hook_out(v.clone())?)))
                .collect::<Result<_, Thrown>>()?,
        ),
        Value::Opaque(_) if speedyspec_php::object_id(&read).is_some() => {
            Value::Opaque(Rc::new(OpaquePhpValue(from_php(&read)?)))
        }
        _ => read.into_owned(),
    })
}
