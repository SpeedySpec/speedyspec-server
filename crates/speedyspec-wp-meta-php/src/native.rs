//! The native functions `meta.php` (the facade) calls, one per WordPress function. The use cases are dispatched
//! through their handlers, each resolved once from the container (Command/CQRS), so PHP may rebind one.
//!
//! Every native function first does `meta.php`'s argument checks, then calls the use case, then rethrows any PHP
//! exception the work met: a callback's (a `MetadataError::Hook` payload), one `wpdb` hid behind `false` or `0`
//! (`take_hook_error()`, as when a `query` callback throws during `add_metadata()`'s insert), or one a port parked.

use std::rc::Rc;

use speedyspec_container::Container;
use speedyspec_cqrs::{BusError, Command, HandlerRef, Query, QueryRef};
use speedyspec_php::ext_php_rs::builders::FunctionBuilder;
use speedyspec_php::native::{self, NativeError};
use speedyspec_php_ext::{Key, Value};
use speedyspec_php_serialize::{PhpValue, maybe_unserialize};
use speedyspec_wp_hooks_contracts::{HookError, HookName, HookValue};
use speedyspec_wp_meta::{
    AddMetadata, ByMidOutcome, DeleteMetadata, DeleteMetadataByMid, FilterDefaultMetadata,
    GetMetadata, GetMetadataByMid, GetMetadataDefault, GetMetadataRaw, GetObjectSubtype,
    IsProtectedMeta, MetadataExists, SanitizeMeta, UpdateMetaCache, UpdateMetadata,
    UpdateMetadataByMid,
};
use speedyspec_wp_meta_contracts::{MetaId, MetadataError, ObjectId, UpdateOutcome};

use crate::adapters::{Parked, exists, is_parked, keep, table_or_false, take_parked};
use crate::args::{a, absint, bytes, int, key, meta_id, meta_type, numeric, text, truthy};
use crate::codec::{Thrown, from_php, string, to_php};

thread_local! {
    /// How many native meta functions are running, one inside another.
    static DEPTH: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

pub(crate) const NATIVE: &str = "SpeedySpec\\WpMeta\\Native\\";

/// The handlers the native functions dispatch through, each resolved once.
pub(crate) struct Handlers {
    pub(crate) add: HandlerRef<AddMetadata>,
    pub(crate) update: HandlerRef<UpdateMetadata>,
    pub(crate) delete: HandlerRef<DeleteMetadata>,
    pub(crate) get: QueryRef<GetMetadata>,
    pub(crate) get_raw: QueryRef<GetMetadataRaw>,
    pub(crate) get_default: QueryRef<GetMetadataDefault>,
    pub(crate) exists: QueryRef<MetadataExists>,
    pub(crate) by_mid: QueryRef<GetMetadataByMid>,
    pub(crate) update_by_mid: HandlerRef<UpdateMetadataByMid>,
    pub(crate) delete_by_mid: HandlerRef<DeleteMetadataByMid>,
    pub(crate) cache: HandlerRef<UpdateMetaCache>,
    pub(crate) sanitize: QueryRef<SanitizeMeta>,
    pub(crate) protected: QueryRef<IsProtectedMeta>,
    pub(crate) subtype: QueryRef<GetObjectSubtype>,
    pub(crate) filter_default: QueryRef<FilterDefaultMetadata>,
}

impl Handlers {
    pub(crate) fn new(c: &Rc<Container>) -> Handlers {
        Handlers {
            add: HandlerRef::new(c.clone()),
            update: HandlerRef::new(c.clone()),
            delete: HandlerRef::new(c.clone()),
            get: QueryRef::new(c.clone()),
            get_raw: QueryRef::new(c.clone()),
            get_default: QueryRef::new(c.clone()),
            exists: QueryRef::new(c.clone()),
            by_mid: QueryRef::new(c.clone()),
            update_by_mid: HandlerRef::new(c.clone()),
            delete_by_mid: HandlerRef::new(c.clone()),
            cache: HandlerRef::new(c.clone()),
            sanitize: QueryRef::new(c.clone()),
            protected: QueryRef::new(c.clone()),
            subtype: QueryRef::new(c.clone()),
            filter_default: QueryRef::new(c.clone()),
        }
    }
}

pub(crate) type R = Result<Value, NativeError>;

/// A native function's body.
pub(crate) type Native = fn(&[Value]) -> R;

/// A hook failure as PHP sees it: the callback's exception rethrown, PHP unwinding left alone, anything else an
/// `Error`.
pub(crate) fn hook_error(hook: HookError) -> NativeError {
    match hook
        .payload
        .as_ref()
        .and_then(|p| p.downcast_ref::<Value>())
    {
        Some(exception) => NativeError::Rethrow(exception.clone()),
        None if hook.payload.is_some() => NativeError::Silent,
        None => NativeError::throw("Error", hook.message),
    }
}

/// A metadata error as PHP sees it.
pub(crate) fn meta_error(e: MetadataError) -> NativeError {
    match e {
        MetadataError::Hook(hook) => hook_error(hook),
        other => NativeError::throw("Error", other.to_string()),
    }
}

/// A conversion PHP failed: its exception, or nothing to throw while PHP unwinds.
pub(crate) fn thrown(t: Thrown) -> NativeError {
    match t.0 {
        Some(exception) => NativeError::Rethrow(exception),
        None => NativeError::Silent,
    }
}

fn bus(e: BusError) -> NativeError {
    NativeError::throw("Error", e.to_string())
}

pub(crate) fn ask<Q: Query<Result = Result<T, MetadataError>>, T>(
    r: &QueryRef<Q>,
    q: Q,
) -> Result<T, NativeError> {
    r.call(q).map_err(bus)?.map_err(meta_error)
}

pub(crate) fn send<C: Command<Result = Result<T, MetadataError>>, T>(
    r: &HandlerRef<C>,
    c: C,
) -> Result<T, NativeError> {
    r.call(c).map_err(bus)?.map_err(meta_error)
}

/// Runs `f` with the area's handlers.
pub(crate) fn h<T>(f: impl FnOnce(&Handlers) -> T) -> Result<T, NativeError> {
    crate::SHARED
        .with(|s| s.borrow().as_ref().map(|s| f(&s.handlers)))
        .ok_or_else(|| NativeError::throw("Error", "the metadata API is not available"))
}

pub(crate) fn php_out(value: &PhpValue) -> R {
    to_php(value).map_err(thrown)
}

pub(crate) fn php_in(value: &Value) -> Result<PhpValue, NativeError> {
    from_php(value).map_err(thrown)
}

fn no() -> R {
    Ok(Value::Bool(false))
}

/// `$object_id` past `! is_numeric( $object_id )`, `absint()` and `! $object_id`.
fn object(value: &Value) -> Option<ObjectId> {
    numeric(value)?;
    Some(ObjectId(absint(value))).filter(|o| o.0 != 0)
}

/// An optional key: `''` (the default) and `null` read the whole entry.
fn optional_key(args: &[Value], i: usize) -> Option<speedyspec_wp_meta_contracts::MetaKey> {
    let v = args.get(i)?;
    match &*v.read() {
        Value::Null => None,
        Value::Str(s) if s.is_empty() => None,
        _ => Some(key(v)),
    }
}

fn add_metadata(x: &[Value]) -> R {
    let Some(meta_type) = meta_type(&a(x, 0)) else {
        return no();
    };
    if !truthy(&a(x, 2)) {
        return no();
    }
    let Some(object) = object(&a(x, 1)) else {
        return no();
    };
    let m = AddMetadata {
        meta_type,
        object,
        key: key(&a(x, 2)),
        value: php_in(&a(x, 3))?,
        unique: truthy(&a(x, 4)),
    };
    php_out(&h(|h| send(&h.add, m))??.to_php())
}

fn update_metadata(x: &[Value]) -> R {
    let Some(meta_type) = meta_type(&a(x, 0)) else {
        return no();
    };
    if !truthy(&a(x, 2)) {
        return no();
    }
    let Some(object) = object(&a(x, 1)) else {
        return no();
    };
    let m = UpdateMetadata {
        meta_type,
        object,
        key: key(&a(x, 2)),
        value: php_in(&a(x, 3))?,
        prev: x.get(4).map(php_in).transpose()?,
    };
    Ok(match h(|h| send(&h.update, m))?? {
        UpdateOutcome::Updated => Value::Bool(true),
        UpdateOutcome::Added(mid) => Value::Int(i64::try_from(mid.0).unwrap_or(i64::MAX)),
        UpdateOutcome::Unchanged | UpdateOutcome::Failed => Value::Bool(false),
    })
}

fn delete_metadata(x: &[Value]) -> R {
    let delete_all = truthy(&a(x, 4));
    let Some(meta_type) = meta_type(&a(x, 0)) else {
        return no();
    };
    if !truthy(&a(x, 2)) || (numeric(&a(x, 1)).is_none() && !delete_all) {
        return no();
    }
    let object = ObjectId(absint(&a(x, 1)));
    if object.0 == 0 && !delete_all {
        return no();
    }
    let m = DeleteMetadata {
        meta_type,
        object,
        key: key(&a(x, 2)),
        value: x.get(3).map(php_in).transpose()?,
        delete_all,
    };
    Ok(Value::Bool(h(|h| send(&h.delete, m))??))
}

pub(crate) fn get_metadata(x: &[Value]) -> R {
    let Some(meta_type) = meta_type(&a(x, 0)) else {
        return no();
    };
    let Some(object) = object(&a(x, 1)) else {
        return no();
    };
    let m = GetMetadata {
        meta_type,
        object,
        key: optional_key(x, 2),
        single: truthy(&a(x, 3)),
    };
    php_out(&h(|h| ask(&h.get, m))??)
}

fn get_metadata_raw(x: &[Value]) -> R {
    let Some(meta_type) = meta_type(&a(x, 0)) else {
        return no();
    };
    let Some(object) = object(&a(x, 1)) else {
        return no();
    };
    let m = GetMetadataRaw {
        meta_type,
        object,
        key: optional_key(x, 2),
        single: truthy(&a(x, 3)),
    };
    match h(|h| ask(&h.get_raw, m))?? {
        Some(value) => php_out(&value),
        None => Ok(Value::Null),
    }
}

fn get_metadata_default(x: &[Value]) -> R {
    let single = truthy(&a(x, 3));
    let Some(meta_type) = meta_type(&a(x, 0)) else {
        return Ok(if single {
            Value::Str(String::new())
        } else {
            Value::Array(Vec::new())
        });
    };
    let m = GetMetadataDefault {
        meta_type,
        object: ObjectId(absint(&a(x, 1))),
        key: key(&a(x, 2)),
        single,
    };
    php_out(&h(|h| ask(&h.get_default, m))??)
}

fn metadata_exists(x: &[Value]) -> R {
    let Some(meta_type) = meta_type(&a(x, 0)) else {
        return no();
    };
    let Some(object) = object(&a(x, 1)) else {
        return no();
    };
    let m = MetadataExists {
        meta_type,
        object,
        key: key(&a(x, 2)),
    };
    Ok(Value::Bool(h(|h| ask(&h.exists, m))??))
}

/// `$wpdb->get_row()`'s object for a meta row, `meta_value` unserialized as `get_metadata_by_mid()` does.
fn row_object(
    meta_type: &speedyspec_wp_meta_contracts::MetaType,
    meta: &speedyspec_wp_meta_contracts::Meta,
) -> R {
    let value = match &meta.value {
        Some(stored) => php_out(&maybe_unserialize(&PhpValue::Str(
            stored.as_bytes().to_vec(),
        )))?,
        None => Value::Null,
    };
    let fields = Value::Array(vec![
        (
            Key::Str(meta_type.id_column().to_owned()),
            Value::Str(meta.mid.0.to_string()),
        ),
        (
            Key::Str(meta_type.object_column()),
            Value::Str(meta.object_id.0.to_string()),
        ),
        (Key::Str("meta_key".into()), string(meta.key.as_bytes())),
        (Key::Str("meta_value".into()), value),
    ]);
    crate::codec::php("SpeedySpec\\WpMeta\\Php\\row", &[fields]).map_err(thrown)
}

fn get_metadata_by_mid(x: &[Value]) -> R {
    let Some(meta_type) = meta_type(&a(x, 0)) else {
        return no();
    };
    let Some(mid) = meta_id(&a(x, 1)) else {
        return no();
    };
    let m = GetMetadataByMid {
        meta_type: meta_type.clone(),
        mid: MetaId(mid),
    };
    match h(|h| ask(&h.by_mid, m))?? {
        ByMidOutcome::Found(meta) => row_object(&meta_type, &meta),
        ByMidOutcome::NotFound => no(),
        ByMidOutcome::Filtered(value) => php_out(&value),
    }
}

fn update_metadata_by_mid(x: &[Value]) -> R {
    let Some(meta_type) = meta_type(&a(x, 0)) else {
        return no();
    };
    let Some(mid) = meta_id(&a(x, 1)) else {
        return no();
    };
    let key = match x.get(3).map(|v| v.read().into_owned()) {
        None | Some(Value::Bool(false)) => None,
        Some(v @ (Value::Str(_) | Value::Bytes(_))) => Some(key(&v)),
        // WordPress runs the short-circuit filter, reads the row, then returns false for a key that is neither
        // false nor a string.
        Some(other) => {
            let hooks = crate::SHARED
                .with(|s| s.borrow().as_ref().map(|s| s.raw_hooks.clone()))
                .ok_or_else(|| NativeError::throw("Error", "the metadata API is not available"))?;
            let name =
                HookName::from(format!("update_{}_metadata_by_mid", meta_type.name()).as_str());
            if !hooks
                .skip_if_empty(&name, speedyspec_wp_hooks_contracts::Count::Filters)
                .map_err(hook_error)?
            {
                let check = hooks
                    .apply_filters(
                        &name,
                        HookValue::Null,
                        vec![
                            Value::Int(i64::try_from(mid).unwrap_or(i64::MAX)),
                            a(x, 2),
                            other,
                        ],
                    )
                    .map_err(hook_error)?;
                if !matches!(&*check.read(), Value::Null) {
                    return Ok(Value::Bool(truthy(&check)));
                }
            }
            let m = GetMetadataByMid {
                meta_type,
                mid: MetaId(mid),
            };
            h(|h| ask(&h.by_mid, m))??;
            return no();
        }
    };
    let m = UpdateMetadataByMid {
        meta_type,
        mid: MetaId(mid),
        value: php_in(&a(x, 2))?,
        key,
    };
    Ok(Value::Bool(h(|h| send(&h.update_by_mid, m))??))
}

fn delete_metadata_by_mid(x: &[Value]) -> R {
    let Some(meta_type) = meta_type(&a(x, 0)) else {
        return no();
    };
    let Some(mid) = meta_id(&a(x, 1)) else {
        return no();
    };
    let m = DeleteMetadataByMid {
        meta_type,
        mid: MetaId(mid),
    };
    Ok(Value::Bool(h(|h| send(&h.delete_by_mid, m))??))
}

/// `$object_ids` as `update_meta_cache()` reads them: a list through `intval`, or a string with everything but
/// digits and commas removed, split on commas.
fn object_ids(value: &Value) -> Vec<ObjectId> {
    let id = |i: i64| ObjectId(u64::try_from(i).unwrap_or(0));
    match &*value.read() {
        Value::Array(items) => items.iter().map(|(_, v)| id(int(v))).collect(),
        other => {
            let kept: Vec<u8> = bytes(other)
                .into_iter()
                .filter(|b| b.is_ascii_digit() || *b == b',')
                .collect();
            kept.split(|b| *b == b',')
                .map(|part| {
                    id(std::str::from_utf8(part)
                        .ok()
                        .and_then(|s| s.parse().ok())
                        .unwrap_or(0))
                })
                .collect()
        }
    }
}

fn update_meta_cache(x: &[Value]) -> R {
    let Some(meta_type) = meta_type(&a(x, 0)) else {
        return no();
    };
    if !truthy(&a(x, 1)) {
        return no();
    }
    let m = UpdateMetaCache {
        meta_type,
        objects: object_ids(&a(x, 1)),
    };
    match h(|h| send(&h.cache, m))?? {
        Some(cache) => php_out(&cache),
        None => no(),
    }
}

fn get_meta_table(x: &[Value]) -> R {
    let table = format!("{}meta", text(&a(x, 0)));
    let name = crate::SHARED
        .with(|s| s.borrow().as_ref().map(|s| s.tables.clone()))
        .map(|tables| speedyspec_wp_db_contracts::Wpdb::table_name(&*tables, &table))
        .unwrap_or_default();
    Ok(table_or_false(name))
}

fn is_protected_meta(x: &[Value]) -> R {
    let m = IsProtectedMeta {
        key: key(&a(x, 0)),
        meta_type: text(&a(x, 1)),
    };
    php_out(&h(|h| ask(&h.protected, m))??)
}

fn sanitize_meta(x: &[Value]) -> R {
    let m = SanitizeMeta {
        key: key(&a(x, 0)),
        value: php_in(&a(x, 1))?,
        object_type: text(&a(x, 2)),
        subtype: text(&a(x, 3)),
    };
    php_out(&h(|h| ask(&h.sanitize, m))??)
}

fn filter_default_metadata(x: &[Value]) -> R {
    let value = a(x, 0);
    let Some(meta_type) = meta_type(&a(x, 4)) else {
        return Ok(value);
    };
    let m = FilterDefaultMetadata {
        value: php_in(&value)?,
        object: ObjectId(u64::try_from(int(&a(x, 1))).unwrap_or(0)),
        key: key(&a(x, 2)),
        single: truthy(&a(x, 3)),
        meta_type,
    };
    php_out(&h(|h| ask(&h.filter_default, m))??)
}

pub(crate) fn object_subtype(
    object_type: &Value,
    object_id: &Value,
) -> Result<String, NativeError> {
    let Some(meta_type) = meta_type(object_type) else {
        return Ok(String::new());
    };
    let id = int(object_id);
    if id < 0 {
        // No object has a negative id, so the switch answers `''`; the filter still gets `(int) $object_id`.
        return negative_subtype(&meta_type, id);
    }
    let m = GetObjectSubtype {
        meta_type,
        object: ObjectId(u64::try_from(int(object_id)).unwrap_or(0)),
    };
    h(|h| ask(&h.subtype, m))?
}

fn negative_subtype(
    meta_type: &speedyspec_wp_meta_contracts::MetaType,
    id: i64,
) -> Result<String, NativeError> {
    let hooks = crate::SHARED
        .with(|s| s.borrow().as_ref().map(|s| s.hooks.clone()))
        .ok_or_else(|| NativeError::throw("Error", "the metadata API is not available"))?;
    let name = HookName::from(format!("get_object_subtype_{}", meta_type.name()).as_str());
    if hooks
        .skip_if_empty(&name, speedyspec_wp_hooks_contracts::Count::Filters)
        .map_err(hook_error)?
    {
        return Ok(String::new());
    }
    let out = hooks
        .apply_filters(
            &name,
            HookValue::Str(String::new()),
            vec![HookValue::Int(id)],
        )
        .map_err(hook_error)?;
    Ok(text(&out))
}

fn get_object_subtype(x: &[Value]) -> R {
    Ok(string(object_subtype(&a(x, 0), &a(x, 1))?.as_bytes()))
}

/// `_doing_it_wrong( $function, __( $message ), $version )`, when WordPress has it.
pub(crate) fn doing_it_wrong(
    function: &str,
    message: &str,
    version: &str,
) -> Result<(), NativeError> {
    if !exists("_doing_it_wrong") {
        return Ok(());
    }
    let message = if exists("__") {
        crate::codec::php("__", &[Value::Str(message.into())]).map_err(thrown)?
    } else {
        Value::Str(message.into())
    };
    crate::codec::php(
        "_doing_it_wrong",
        &[
            Value::Str(function.into()),
            message,
            Value::Str(version.into()),
        ],
    )
    .map_err(thrown)?;
    Ok(())
}

/// A hook name from a value's bytes.
pub(crate) fn hook_name(bytes: Vec<u8>) -> HookName {
    match String::from_utf8(bytes) {
        Ok(s) => HookName::from(HookValue::Str(s)),
        Err(e) => HookName::from(HookValue::Bytes(e.into_bytes())),
    }
}

/// Clears what a call from Rust may have left, runs `f`, then rethrows the first failure the work met.
///
/// A native function PHP calls while another one runs (from a hook callback) keeps the outer call's parked failure
/// for it; the outermost call starts clean.
fn guarded(f: Native, args: &[Value]) -> R {
    let depth = DEPTH.with(|d| {
        d.set(d.get() + 1);
        d.get()
    });
    let _ = speedyspec_wp_db_contracts::take_hook_error();
    let outer = take_parked().filter(|_| depth > 1);
    let result = f(args);
    let parked = take_parked();
    if let Some(outer) = outer {
        keep(outer);
    }
    DEPTH.with(|d| d.set(d.get() - 1));
    let hidden = speedyspec_wp_db_contracts::take_hook_error();
    match result {
        Err(NativeError::Rethrow(exception)) => Err(NativeError::Rethrow(exception)),
        other => match (parked, hidden) {
            // A port's failure stopped the use case: it is what PHP sees.
            (Some(Parked::Exception(exception)), _) => Err(NativeError::Rethrow(exception)),
            (Some(Parked::Failure { class, message }), _) => {
                Err(NativeError::throw(class, message))
            }
            // `wpdb` answered false or 0 where a `query` callback threw: the exception still reaches PHP.
            (None, Some(hook)) => Err(hook_error(hook)),
            (None, None) => other,
        },
    }
}

/// Stops a native function once a port has parked a failure; [`guarded`] rethrows what was parked.
pub(crate) fn stop_if_parked() -> Result<(), NativeError> {
    if is_parked() {
        return Err(NativeError::Silent);
    }
    Ok(())
}

pub(crate) fn functions() -> Vec<FunctionBuilder<'static>> {
    let mut entries: Vec<(&str, Native)> = vec![
        ("add_metadata", add_metadata),
        ("update_metadata", update_metadata),
        ("delete_metadata", delete_metadata),
        ("get_metadata", get_metadata),
        ("get_metadata_raw", get_metadata_raw),
        ("get_metadata_default", get_metadata_default),
        ("metadata_exists", metadata_exists),
        ("get_metadata_by_mid", get_metadata_by_mid),
        ("update_metadata_by_mid", update_metadata_by_mid),
        ("delete_metadata_by_mid", delete_metadata_by_mid),
        ("update_meta_cache", update_meta_cache),
        ("_get_meta_table", get_meta_table),
        ("is_protected_meta", is_protected_meta),
        ("sanitize_meta", sanitize_meta),
        ("filter_default_metadata", filter_default_metadata),
        ("get_object_subtype", get_object_subtype),
    ];
    entries.extend(crate::register::entries());
    entries.extend(crate::lazy::entries());
    entries
        .into_iter()
        .map(|(name, f)| {
            native::function(&format!("{NATIVE}{name}"), move |args: &[Value]| {
                guarded(f, args)
            })
        })
        .collect()
}
