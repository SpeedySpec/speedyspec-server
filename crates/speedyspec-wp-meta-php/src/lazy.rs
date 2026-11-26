//! The natives behind `WP_Metadata_Lazyloader` (`lazyloader.php`): each PHP instance owns one Rust [`LazyLoader`],
//! numbered for the request, as each WordPress instance owns its `$pending_objects`.
//!
//! The class itself hooks and unhooks `lazyload_meta_callback` (a PHP callable), so these natives only queue, load
//! and reset.

use std::rc::Rc;

use speedyspec_php::native::NativeError;
use speedyspec_php_ext::Value;
use speedyspec_php_serialize::PhpValue;
use speedyspec_wp_meta::{LazyError, LazyLoader};
use speedyspec_wp_meta_contracts::{MetaType, Metadata, ObjectId};

use crate::args::{a, int, text};
use crate::native::{Native, R};

fn lazy_error(e: LazyError) -> NativeError {
    match e.payload() {
        Some(payload) => match payload.downcast_ref::<Value>() {
            Some(exception) => NativeError::Rethrow(exception.clone()),
            None => NativeError::Silent,
        },
        None => NativeError::throw("Error", e.to_string()),
    }
}

fn loader(x: &[Value]) -> Result<Rc<LazyLoader>, NativeError> {
    let index = usize::try_from(int(&a(x, 0))).ok();
    crate::SHARED
        .with(|s| {
            let shared = s.borrow();
            let shared = shared.as_ref()?;
            let loaders = shared.loaders.borrow();
            loaders.get(index?).cloned()
        })
        .ok_or_else(|| NativeError::throw("Error", "the metadata lazy loader is not available"))
}

fn ids(value: &Value) -> Vec<ObjectId> {
    match &*value.read() {
        Value::Array(items) => items
            .iter()
            .map(|(_, v)| ObjectId(u64::try_from(int(v)).unwrap_or(0)))
            .collect(),
        _ => Vec::new(),
    }
}

/// A new loader for a new `WP_Metadata_Lazyloader`; its number.
fn lazy_new(_: &[Value]) -> R {
    crate::SHARED
        .with(|s| {
            let shared = s.borrow();
            let shared = shared.as_ref()?;
            let metadata: Rc<dyn Metadata> = shared.api.clone();
            let mut loaders = shared.loaders.borrow_mut();
            loaders.push(Rc::new(LazyLoader::new(shared.hooks.clone(), metadata)));
            Some(Value::Int(
                i64::try_from(loaders.len() - 1).unwrap_or(i64::MAX),
            ))
        })
        .ok_or_else(|| NativeError::throw("Error", "the metadata API is not available"))
}

/// `queue_objects()` after the class has hooked its callback: remembers the ids and fires
/// `metadata_lazyloader_queued_objects` with `$object_ids`, `$object_type` and the loader object.
fn lazy_queue(x: &[Value]) -> R {
    loader(x)?
        .queue_with(&text(&a(x, 1)), &ids(&a(x, 2)), a(x, 3))
        .map_err(lazy_error)?;
    Ok(Value::Null)
}

/// `reset_queue()`'s queue half.
fn lazy_reset(x: &[Value]) -> R {
    loader(x)?.reset(&text(&a(x, 1))).map_err(lazy_error)?;
    Ok(Value::Null)
}

/// `! empty( $this->pending_objects[ $meta_type ] )`.
fn lazy_pending(x: &[Value]) -> R {
    Ok(Value::Bool(!loader(x)?.pending(&text(&a(x, 1))).is_empty()))
}

/// `lazyload_meta_callback()`'s load: every queued id, plus the object being read (argument 2) when it is not
/// queued, through `update_meta_cache()`.
fn lazy_load(x: &[Value]) -> R {
    let Some(meta_type) = MetaType::new(&text(&a(x, 1))) else {
        return Ok(Value::Null);
    };
    let object = u64::try_from(int(&a(x, 2))).ok().map(ObjectId);
    loader(x)?
        .load_for_object(&meta_type, PhpValue::Null, object)
        .map_err(lazy_error)?;
    Ok(Value::Null)
}

pub(crate) fn entries() -> Vec<(&'static str, Native)> {
    vec![
        ("lazy_new", lazy_new),
        ("lazy_queue", lazy_queue),
        ("lazy_reset", lazy_reset),
        ("lazy_pending", lazy_pending),
        ("lazy_load", lazy_load),
    ]
}
