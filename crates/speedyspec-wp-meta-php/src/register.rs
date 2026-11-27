//! `register_meta()` and the functions over `$wp_meta_keys`, native.
//!
//! `$wp_meta_keys` stays PHP's global (Registry): WordPress's tests and plugins read and reset it directly, so the
//! natives write it through small PHP helpers (`meta.php`) and read it live ([`PhpMetaKeys`]). `register_meta()` runs
//! the carried order: [`prepare_registration`], the `register_meta_args` filter, `MetadataApi::resolve_registration`
//! (so an `is_protected_meta` callback's exception is rethrown), then every returned filter is added, also when the
//! registration is rejected, as WordPress adds them before its default-value check gives up.

use speedyspec_php::native::NativeError;
use speedyspec_php_ext::{Key, Value};
use speedyspec_php_serialize::PhpValue;
use speedyspec_php_serialize::semantics::to_array;
use speedyspec_wp_hooks_contracts::{CallbackId, Count, HasFilterResult, HookName, HookValue};
use speedyspec_wp_hooks_php::{php_callback, unique_id};
use speedyspec_wp_meta::{FilterToAdd, Registration, filters_to_remove, prepare_registration};
use speedyspec_wp_meta_contracts::MetaKeys;

use crate::adapters::{
    PhpCallables, PhpDefaultSchemaCheck, PhpMetaKeys, PhpRevisionSupport, truthy,
};
use crate::args::{a, key, text};
use crate::codec::{from_php_live, php, to_php};
use crate::native::{
    Native, R, doing_it_wrong, get_metadata, hook_error, hook_name, meta_error, object_subtype,
    stop_if_parked, thrown,
};

const HELPERS: &str = "SpeedySpec\\WpMeta\\Php\\";

fn helper(name: &str, args: &[Value]) -> R {
    php(&format!("{HELPERS}{name}"), args).map_err(thrown)
}

fn shared() -> Result<std::rc::Rc<crate::Shared>, NativeError> {
    crate::SHARED
        .with(|s| s.borrow().clone())
        .ok_or_else(|| NativeError::throw("Error", "the metadata API is not available"))
}

/// `register_meta()`'s `$defaults`, as the `register_meta_args` filter receives them.
fn defaults() -> Value {
    let s = |v: &str| Value::Str(v.into());
    Value::Array(
        [
            ("object_subtype", s("")),
            ("type", s("string")),
            ("label", s("")),
            ("description", s("")),
            ("default", s("")),
            ("single", Value::Bool(false)),
            ("sanitize_callback", Value::Null),
            ("auth_callback", Value::Null),
            ("show_in_rest", Value::Bool(false)),
            ("revisions_enabled", Value::Bool(false)),
        ]
        .into_iter()
        .map(|(k, v)| (Key::Str(k.into()), v))
        .collect(),
    )
}

fn add_filters(shared: &crate::Shared, filters: Vec<FilterToAdd>) -> Result<(), NativeError> {
    for filter in filters {
        let callback = to_php(&filter.callback).map_err(thrown)?;
        shared
            .raw_hooks
            .add_filter(
                &hook_name(filter.hook),
                php_callback(callback),
                &HookValue::Int(filter.priority),
                i64::from(filter.accepted_args),
            )
            .map_err(hook_error)?;
    }
    Ok(())
}

/// Hooks `filter_default_metadata` on `default_{$object_type}_metadata` (10, 5) unless it is there.
fn add_default_filter(shared: &crate::Shared, object_type: &str) -> Result<(), NativeError> {
    let name = HookName::from(format!("default_{object_type}_metadata").as_str());
    let callback = Value::Str("filter_default_metadata".into());
    let id = CallbackId(unique_id(&callback));
    if shared
        .raw_hooks
        .has_filter(&name, Some(&id), None)
        .map_err(hook_error)?
        == HasFilterResult::No
    {
        shared
            .raw_hooks
            .add_filter(&name, php_callback(callback), &HookValue::Int(10), 5)
            .map_err(hook_error)?;
    }
    Ok(())
}

fn register_meta(x: &[Value]) -> R {
    let shared = shared()?;
    helper("keys_init", &[])?;
    let object_type = text(&a(x, 0));
    let meta_key = key(&a(x, 1));
    let deprecated = x
        .get(3)
        .map(from_php_live)
        .filter(|d| !matches!(d, PhpValue::Null));
    let mut prepared = prepare_registration(from_php_live(&a(x, 2)), deprecated, &PhpCallables);
    // An `is_callable()` that threw stops register_meta() here, as it does WordPress's.
    stop_if_parked()?;
    let filter = HookName::from("register_meta_args");
    if !shared
        .raw_hooks
        .skip_if_empty(&filter, Count::Filters)
        .map_err(hook_error)?
    {
        let args = to_php(&PhpValue::Array(prepared.args.clone())).map_err(thrown)?;
        let filtered = shared
            .raw_hooks
            .apply_filters(&filter, args, vec![defaults(), a(x, 0), a(x, 1)])
            .map_err(hook_error)?;
        // `wp_parse_args()` takes an object's properties or a scalar as a one-element array.
        prepared.args = match from_php_live(&filtered) {
            PhpValue::Array(items) => items,
            other => to_array(other),
        };
    }
    let registration = shared
        .api
        .resolve_registration(
            &object_type,
            &meta_key,
            prepared,
            &PhpRevisionSupport,
            &PhpDefaultSchemaCheck,
            &PhpCallables,
        )
        .map_err(meta_error)?;
    // So does a port that threw while the registration was resolved (`post_type_supports()`, the schema check).
    stop_if_parked()?;
    match registration {
        Registration::Registered {
            key,
            filters,
            add_default_filter: default,
        } => {
            add_filters(&shared, filters)?;
            if default {
                add_default_filter(&shared, &object_type)?;
            }
            let args = to_php(&PhpValue::Array(key.args)).map_err(thrown)?;
            helper(
                "keys_put",
                &[a(x, 0), Value::Str(key.subtype), a(x, 1), args],
            )?;
            Ok(Value::Bool(true))
        }
        Registration::HooksOnly {
            filters,
            add_default_filter: default,
        } => {
            add_filters(&shared, filters)?;
            if default {
                add_default_filter(&shared, &object_type)?;
            }
            Ok(Value::Bool(false))
        }
        Registration::Rejected {
            message,
            version,
            filters,
        } => {
            add_filters(&shared, filters)?;
            doing_it_wrong("register_meta", &message, version)?;
            Ok(Value::Bool(false))
        }
    }
}

fn unregister_meta_key(x: &[Value]) -> R {
    let shared = shared()?;
    let object_type = text(&a(x, 0));
    let subtype = text(&a(x, 2));
    let Some(found) = PhpMetaKeys.find(&object_type, &subtype, &key(&a(x, 1))) else {
        return Ok(Value::Bool(false));
    };
    for (hook, callback) in filters_to_remove(&object_type, &found, &PhpCallables) {
        let callback = to_php(&callback).map_err(thrown)?;
        shared
            .raw_hooks
            .remove_filter(
                &hook_name(hook),
                CallbackId(unique_id(&callback)),
                &HookValue::Int(10),
            )
            .map_err(hook_error)?;
    }
    helper("keys_remove", &[a(x, 0), Value::Str(subtype), a(x, 1)])?;
    Ok(Value::Bool(true))
}

fn registered(x: &[Value]) -> R {
    let found = PhpMetaKeys.find(&text(&a(x, 0)), &text(&a(x, 2)), &key(&a(x, 1)));
    Ok(Value::Bool(found.is_some()))
}

/// `$wp_meta_keys[ $object_type ][ $object_subtype ]` as PHP holds it, or an empty array.
fn keys_of(object_type: &str, subtype: &str) -> Value {
    helper(
        "keys_get",
        &[Value::Str(object_type.into()), Value::Str(subtype.into())],
    )
    .unwrap_or(Value::Array(Vec::new()))
}

fn get_registered_meta_keys(x: &[Value]) -> R {
    helper("keys_get", &[a(x, 0), Value::Str(text(&a(x, 1)))])
}

fn get_registered_metadata(x: &[Value]) -> R {
    let object_type = text(&a(x, 0));
    let mut subtype = object_subtype(&a(x, 0), &a(x, 1))?;
    let meta_key = a(x, 2);
    if truthy(&meta_key) {
        let meta_key_bytes = key(&meta_key);
        let exists = |subtype: &str| PhpMetaKeys.find(&object_type, subtype, &meta_key_bytes);
        if !subtype.is_empty() && subtype != "0" && exists(&subtype).is_none() {
            subtype = String::new();
        }
        let Some(found) = exists(&subtype) else {
            return Ok(Value::Bool(false));
        };
        let single = found.arg("single").cloned().unwrap_or(PhpValue::Null);
        let single = to_php(&single).map_err(thrown)?;
        return get_metadata(&[a(x, 0), a(x, 1), meta_key, single]);
    }
    let data = get_metadata(&[a(x, 0), a(x, 1)])?;
    if !truthy(&data) {
        return Ok(Value::Array(Vec::new()));
    }
    let mut meta_keys = keys_of(&object_type, "");
    if !subtype.is_empty() && subtype != "0" {
        meta_keys =
            php("array_merge", &[meta_keys, keys_of(&object_type, &subtype)]).map_err(thrown)?;
    }
    php("array_intersect_key", &[data, meta_keys]).map_err(thrown)
}

fn allowed_list(x: &[Value]) -> R {
    php("array_intersect_key", &[a(x, 0), a(x, 1)]).map_err(thrown)
}

pub(crate) fn entries() -> Vec<(&'static str, Native)> {
    vec![
        ("register_meta", register_meta),
        ("unregister_meta_key", unregister_meta_key),
        ("registered_meta_key_exists", registered),
        ("get_registered_meta_keys", get_registered_meta_keys),
        ("get_registered_metadata", get_registered_metadata),
        ("_wp_register_meta_args_allowed_list", allowed_list),
    ]
}
