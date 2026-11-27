//! PHP values and the post type rules' values (`PhpValue`), both ways (Adapter).
//!
//! The rules read a post type's arguments and properties as `PhpValue`s. A PHP object crosses in one of two ways:
//!
//! - a `stdClass` (the `cap` and `labels` objects, a status, an object passed as `labels` or `supports`) is copied
//!   with its properties, so the rules can read inside it;
//! - any other object (a meta box callback's closure, `[$this, 'method']`, a REST controller) stays the object
//!   itself: a marker naming a slot in a table of live values, so its identity survives the trip. The table is
//!   emptied when the outermost native function returns, so the objects it holds are released then.
//!
//! [`to_php`] turns a `PhpValue` back into a PHP value: a marker is the very object it names, a copied `stdClass` is
//! cast from its properties (`(object) $properties`), anything else is built by PHP's `unserialize()`.

use std::any::Any;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use speedyspec_php::Callable;
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

/// Forgets the live values: the native call that made them is over.
pub(crate) fn clear_live() {
    LIVE.with(|l| *l.borrow_mut() = Live::default());
}

/// Forgets the `posts` entries' renderings ([`post_entry`]): the request that made them is over.
pub(crate) fn clear_renderings() {
    RENDERINGS.with(|r| *r.borrow_mut() = Renderings::default());
}

/// At a request's start, drops nothing a previous request left: had its end not cleared the renderings, their PHP
/// values would belong to a heap already freed, so they are forgotten rather than released.
pub(crate) fn forget_renderings() {
    RENDERINGS.with(|r| std::mem::forget(std::mem::take(&mut *r.borrow_mut())));
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
        PhpValue::Object { class, properties } if class.eq_ignore_ascii_case("WP_Post") => {
            post_object(properties)?
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

/// A PHP value as the read use cases take it: like [`from_php`], but every object (a `WP_Post` included) is copied
/// with its class and public properties, so the use cases can read inside it. A nested object that is not a
/// `stdClass` stays live (see the module's docs).
pub fn copy_php(value: &Value) -> Result<PhpValue, Thrown> {
    let read = value.read();
    match &*read {
        Value::Opaque(any) if any.downcast_ref::<OpaquePhpValue>().is_none() => {
            if speedyspec_php::object_id(&read).is_none() {
                return Ok(keep(&read));
            }
            let class = php("get_class", std::slice::from_ref(&*read))?;
            let class = bytes_of(&class)
                .map(|b| String::from_utf8_lossy(&b).into_owned())
                .unwrap_or_default();
            let properties = object_vars(&read)?
                .iter()
                .map(|(k, v)| Ok((property_key(k), convert(v, 1)?)))
                .collect::<Result<_, Thrown>>()?;
            Ok(PhpValue::Object { class, properties })
        }
        Value::Array(items) => Ok(PhpValue::Array(
            items
                .iter()
                .map(|(k, v)| Ok((php_key(k), copy_php(v)?)))
                .collect::<Result<_, Thrown>>()?,
        )),
        _ => convert(&read, 0),
    }
}

/// A `WP_Post` (or any other class with public properties) as PHP builds it from copied properties: `new
/// WP_Post( (object) $properties )` for `WP_Post`, as `WP_Post::get_instance()` makes one.
pub(crate) fn post_object(properties: &[(PhpKey, PhpValue)]) -> Result<Value, Thrown> {
    let properties = pairs(properties, |k| match k {
        PhpKey::Int(i) => Key::Str(i.to_string()),
        PhpKey::Str(s) => Key::Str(String::from_utf8_lossy(s).into_owned()),
    })?;
    helper("post", &[properties])
}

/// [`post_object`] over properties it may take: each string moves into the PHP value instead of being copied (as
/// bytes: PHP's strings are bytes either way), and the helper is resolved once per request ([`Callable`]). The
/// properties `WP_Post::get_instance()` builds are `WP_Post`'s declared ones in class order; those go as a list of
/// values, so no key is written or hashed.
pub(crate) fn post_object_owned(properties: Vec<(PhpKey, PhpValue)>) -> Result<Value, Thrown> {
    let declared = speedyspec_wp_posts::PostEntry::declared();
    if properties.len() == declared.len()
        && properties
            .iter()
            .zip(declared)
            .all(|((k, _), d)| matches!(k, PhpKey::Str(k) if k == d.as_bytes()))
    {
        // Exactly the declared properties in class order: their values alone, which the helper assigns by name.
        let values = Value::Array(
            properties
                .into_iter()
                .enumerate()
                .map(|(i, (_, v))| {
                    let value = match v {
                        PhpValue::Str(s) => Value::Bytes(s),
                        other => to_php(&other)?,
                    };
                    Ok((Key::Int(i as i64), value))
                })
                .collect::<Result<_, Thrown>>()?,
        );
        return POST_DECLARED
            .with(|helper| helper.call(std::slice::from_ref(&values)))
            .map_err(|failure| Thrown(failure.exception));
    }
    let properties = Value::Array(
        properties
            .into_iter()
            .map(|(k, v)| {
                let key = match k {
                    PhpKey::Int(i) => Key::Str(i.to_string()),
                    PhpKey::Str(s) => Key::Str(
                        String::from_utf8(s)
                            .unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned()),
                    ),
                };
                let value = match v {
                    PhpValue::Str(s) => Value::Bytes(s),
                    other => to_php(&other)?,
                };
                Ok((key, value))
            })
            .collect::<Result<_, Thrown>>()?,
    );
    POST_HELPER
        .with(|helper| helper.call(std::slice::from_ref(&properties)))
        .map_err(|failure| Thrown(failure.exception))
}

/// `new WP_Post( $entry )` over a plain raw `posts` entry the use case lent
/// ([`PostObject::Entry`](speedyspec_wp_posts::PostObject::Entry)), with no property copied.
///
/// The entry is an immutable `PhpValue` the object cache holds behind a shared handle. Its declared properties'
/// values are written to PHP once, as a list kept for the rest of the request ([`Renderings`], Identity Map keyed by
/// the handle); each `WP_Post` built over it then shares PHP's strings by reference count, as WordPress's own
/// `new WP_Post( clone $entry )` does. A replaced or deleted entry drops its handle, so its rendering is never
/// reused.
pub(crate) fn post_entry(entry: &Value) -> Result<Value, Thrown> {
    let Value::Opaque(any) = entry else {
        return Ok(Value::Null);
    };
    let Some(OpaquePhpValue(PhpValue::Object { properties, .. })) =
        any.downcast_ref::<OpaquePhpValue>()
    else {
        return Ok(Value::Null);
    };
    let at = Rc::as_ptr(any).cast::<()>() as usize;
    let kept = RENDERINGS.with(|r| r.borrow().get(at));
    let values = match kept {
        Some(values) => values,
        None => {
            let list = Value::Array(
                properties
                    .iter()
                    .enumerate()
                    .map(|(i, (_, v))| {
                        let value = match v {
                            PhpValue::Str(s) => Value::Bytes(s.clone()),
                            other => to_php(other)?,
                        };
                        Ok((Key::Int(i as i64), value))
                    })
                    .collect::<Result<_, Thrown>>()?,
            );
            // Written once: what comes back is PHP's own array, passed through by reference from now on.
            let values = VALUES
                .with(|helper| helper.call(std::slice::from_ref(&list)))
                .map_err(|failure| Thrown(failure.exception))?;
            RENDERINGS.with(|r| r.borrow_mut().keep(at, Rc::downgrade(any), values.clone()));
            values
        }
    };
    POST_DECLARED
        .with(|helper| helper.call(std::slice::from_ref(&values)))
        .map_err(|failure| Thrown(failure.exception))
}

/// The PHP lists [`post_entry`] wrote, by the address of the entry's handle. The weak handle keeps the address from
/// being reused while the rendering is kept, and tells a dropped entry from a live one.
///
/// Dropped entries' renderings are pruned when the map has doubled since the last prune (512 at least), so a loop
/// over many live posts (a sitemap, an export, WP-CLI) costs amortized O(1) per insert, not a scan each time.
///
/// An object cache that hands back a fresh handle on every read (a backend that decodes each `get`) never hits
/// here: every warm read then writes its values again, and the dead renderings wait for the next prune. That is
/// correct, only slower.
struct Renderings<V> {
    by_entry: HashMap<usize, (Weak<dyn Any>, V)>,
    /// The size that triggers the next prune.
    prune_at: usize,
    /// How many prunes ran, for the tests.
    prunes: usize,
}

impl<V> Default for Renderings<V> {
    fn default() -> Self {
        Renderings {
            by_entry: HashMap::new(),
            prune_at: PRUNE_FLOOR,
            prunes: 0,
        }
    }
}

/// The smallest [`Renderings`] map that is ever pruned.
const PRUNE_FLOOR: usize = 512;

impl<V: Clone> Renderings<V> {
    fn get(&self, at: usize) -> Option<V> {
        let (entry, values) = self.by_entry.get(&at)?;
        (entry.strong_count() > 0).then(|| values.clone())
    }

    fn keep(&mut self, at: usize, entry: Weak<dyn Any>, values: V) {
        if self.by_entry.len() >= self.prune_at {
            self.by_entry.retain(|_, (e, _)| e.strong_count() > 0);
            self.prune_at = (self.by_entry.len() * 2).max(PRUNE_FLOOR);
            self.prunes += 1;
        }
        self.by_entry.insert(at, (entry, values));
    }
}

thread_local! {
    static RENDERINGS: RefCell<Renderings<Value>> = RefCell::new(Renderings::default());
    /// The facade's `values()` helper (an array as PHP holds it), resolved once per request.
    static VALUES: Callable = Callable::new(Value::Str(format!("{HELPERS}values")));
    /// The facade's `post()` helper, resolved once per request.
    static POST_HELPER: Callable = Callable::new(Value::Str(format!("{HELPERS}post")));
    /// The facade's `post_declared()` helper, resolved once per request.
    static POST_DECLARED: Callable = Callable::new(Value::Str(format!("{HELPERS}post_declared")));
}

/// Whether a hook value carries a `PhpValue` PHP cannot read as it is.
fn carries_php_values(value: &Value) -> bool {
    match value {
        Value::Opaque(any) => any.is::<OpaquePhpValue>(),
        Value::Array(items) => items.iter().any(|(_, v)| carries_php_values(v)),
        _ => false,
    }
}

/// A hook value with every carried `PhpValue` (an object the use cases built, a `WP_Post`) made a PHP value, for a
/// PHP callback to read.
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

/// A hook's answer as the use cases read it: every live PHP object in it carried as a `PhpValue` ([`from_php`]: a
/// `stdClass` copied with its properties, any other object kept live, so handing it back to PHP gives the very
/// object the callback returned).
pub(crate) fn hook_out(value: Value) -> Result<Value, Thrown> {
    let read = value.read();
    Ok(match &*read {
        Value::Array(items) => Value::Array(
            items
                .iter()
                .map(|(k, v)| Ok((k.clone(), hook_out(v.clone())?)))
                .collect::<Result<_, Thrown>>()?,
        ),
        Value::Opaque(any)
            if !any.is::<OpaquePhpValue>() && speedyspec_php::object_id(&read).is_some() =>
        {
            Value::Opaque(std::rc::Rc::new(OpaquePhpValue(from_php(&read)?)))
        }
        _ => read.into_owned(),
    })
}

/// A cached value as the read use cases read it: every live PHP object in it carried as a `PhpValue` copied with its
/// class and properties ([`copy_php`]), so a `WP_Post` or `stdClass` another caller cached can be read as a post.
pub(crate) fn cache_out(value: &Value) -> Result<Value, Thrown> {
    let read = value.read();
    Ok(match &*read {
        Value::Array(items) => Value::Array(
            items
                .iter()
                .map(|(k, v)| Ok((k.clone(), cache_out(v)?)))
                .collect::<Result<_, Thrown>>()?,
        ),
        Value::Opaque(any)
            if !any.is::<OpaquePhpValue>() && speedyspec_php::object_id(&read).is_some() =>
        {
            Value::Opaque(std::rc::Rc::new(OpaquePhpValue(copy_php(&read)?)))
        }
        _ => read.into_owned(),
    })
}

#[cfg(test)]
mod tests {
    use std::any::Any;
    use std::rc::Rc;

    use super::Renderings;

    /// 20,000 renderings that all stay alive: the prunes find nothing to drop, and run O(log n) times.
    #[test]
    fn pruning_is_amortized_over_live_entries() {
        let mut r: Renderings<u32> = Renderings::default();
        let entries: Vec<Rc<dyn Any>> = (0..20_000).map(|_| Rc::new(0u8) as Rc<dyn Any>).collect();
        for (i, e) in entries.iter().enumerate() {
            let at = Rc::as_ptr(e).cast::<()>() as usize;
            r.keep(at, Rc::downgrade(e), i as u32);
        }
        assert_eq!(r.by_entry.len(), 20_000);
        // 512, 1024, 2048, … 16384: six prunes.
        assert!(r.prunes <= 7, "{} prunes", r.prunes);
        let e = &entries[19_999];
        assert_eq!(r.get(Rc::as_ptr(e).cast::<()>() as usize), Some(19_999));
    }

    /// Dropped entries are pruned, and a dropped entry's rendering is never answered.
    #[test]
    fn dropped_entries_are_pruned() {
        let mut r: Renderings<u32> = Renderings::default();
        let mut kept = Vec::new();
        for i in 0..2_000u32 {
            let e: Rc<dyn Any> = Rc::new(i);
            let at = Rc::as_ptr(&e).cast::<()>() as usize;
            r.keep(at, Rc::downgrade(&e), i);
            if i % 100 == 0 {
                kept.push((at, e));
            } else {
                assert_eq!(r.get(at), Some(i));
                drop(e);
                assert_eq!(r.get(at), None);
            }
        }
        assert!(r.by_entry.len() < 1_024, "{}", r.by_entry.len());
        for (at, e) in &kept {
            assert_eq!(r.get(*at), e.downcast_ref::<u32>().copied());
        }
    }
}
