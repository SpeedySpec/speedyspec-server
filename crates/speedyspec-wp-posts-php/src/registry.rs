//! The native functions `post-types.php` (the facade) calls, one per WordPress function: post.php's post type and
//! post status registries (7.2-RC2 lines 20-834 and 1467-2578).
//!
//! The rules are the Rust ones (`speedyspec-wp-posts`): the name check, `WP_Post_Type`'s argument resolution
//! (`set_props()`), the capabilities, the labels, `add_supports()`, the unregistration and its rebuilt meta
//! capabilities, the status defaults and the list filter. This adapter does what the rules leave to it (the spec's
//! *Carried from phase 2a*):
//!
//! - `wp_parse_args()` on query-string or object args, `sanitize_key()` with its filter, and the `is_scalar()` and
//!   `is_string()` checks on names;
//! - the `register_post_type_args`, `register_{$post_type}_post_type_args` and `post_type_labels_{$post_type}`
//!   filters, the `registered_post_type` and `unregistered_post_type` actions, and the viewable filters, through the
//!   Hook API with PHP's own values (the filters see the live objects);
//! - applying the rules' results to `$_wp_post_type_features` and `$post_type_meta_caps` (all of the rebuilt map on
//!   unregistering), and building the `WP_Post_Type` object for `$wp_post_types`.
//!
//! `WP_Post_Type` stays WordPress's class: the object is made without its constructor and given the resolved
//! properties, and `set_props()` and `add_supports()` stay its own methods for a caller that uses them. The steps
//! that belong to unported seams stay the class's methods too and are called as WordPress calls them:
//! `add_rewrite_rules()`/`remove_rewrite_rules()` (rewrite), `register_meta_boxes()`/`unregister_meta_boxes()`,
//! `add_hooks()`/`remove_hooks()`, and `register_taxonomies()`/`unregister_taxonomies()` (taxonomy).
//!
//! Approximations, none of them recorded:
//! - a `stdClass` inside the args crosses as a copy (another object stays itself), so a `register_post_type_args`
//!   callback that hands in a `stdClass` gets an equal object back on the post type, not the same one;
//! - `create_initial_post_types()` translates its labels before it registers anything; WordPress translates each
//!   type's as it registers it, so a `gettext` callback sees the same strings in another order.

use std::cell::Cell;
use std::rc::Rc;

use speedyspec_php::ext_php_rs::builders::FunctionBuilder;
use speedyspec_php::native::{self, NativeError};
use speedyspec_php_ext::{Key, Value};
use speedyspec_php_serialize::semantics::{get, is_empty, key_to_string, to_array, to_php_string};
use speedyspec_php_serialize::{PhpKey, PhpValue};
use speedyspec_wp_hooks_contracts::{Count, HookApi, HookError, HookName, HookValue};
use speedyspec_wp_posts::{
    FilterOperator, InitialCall, InitialOptions, LabelPair, LabelSource, ListItem,
    NAME_LENGTH_VERSION, PostTypePorts, add_supports, apply_filtered_labels, custom_object_labels,
    filter_object_list, initial_calls, is_post_status_viewable, is_post_type_viewable,
    merge_filtered_labels, meta_capabilities, post_type_capabilities, post_type_labels,
    post_types_by_support, resolve_post_status, resolve_post_type, unregistration,
};
use speedyspec_wp_posts_contracts::{PostStatusName, PostTypeError, PostTypeName};

use crate::adapters::{
    PhpPostTypeSupports, PhpPostTypes, PhpRewriteContext, PhpTitleSanitizer, PhpTranslator, exists,
    global_entry, is_parked, key_bytes, post_status_from, post_type_from, restore, take_parked,
    truthy,
};
use crate::codec::{
    Thrown, from_php, helper, is_object, method, object_vars, php, property, string, to_php,
};

pub(crate) const NATIVE: &str = "SpeedySpec\\WpPosts\\Native\\";

/// `register_post_type()`'s message for a bad name.
const NAME_LENGTH: &str = "Post type names must be between 1 and 20 characters in length.";

thread_local! {
    /// How many native post type functions are running, one inside another.
    static DEPTH: Cell<usize> = const { Cell::new(0) };
}

pub(crate) type R = Result<Value, NativeError>;

/// A native function's body.
pub(crate) type Native = fn(&[Value]) -> R;

/// The `i`th argument, `null` when it was not passed.
pub(crate) fn a(args: &[Value], i: usize) -> Value {
    args.get(i).cloned().unwrap_or(Value::Null)
}

/// A PHP failure as PHP sees it: its exception rethrown, or nothing to throw while PHP unwinds.
pub(crate) fn thrown(t: Thrown) -> NativeError {
    match t.0 {
        Some(exception) => NativeError::Rethrow(exception),
        None => NativeError::Silent,
    }
}

/// A hook failure as PHP sees it: the callback's exception rethrown, an error WordPress's own code would raise
/// ([`speedyspec_wp_posts::PhpThrow`]) thrown with its class and message, PHP unwinding left alone, anything else an
/// `Error`.
pub(crate) fn hook_error(hook: HookError) -> NativeError {
    if let Some(thrown) = hook
        .payload
        .as_ref()
        .and_then(|p| p.downcast_ref::<speedyspec_wp_posts::PhpThrow>())
    {
        return NativeError::throw(thrown.class, thrown.message.clone());
    }
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

/// Stops once a port has parked an exception; [`guarded`] rethrows it.
fn stop_if_parked() -> Result<(), NativeError> {
    if is_parked() {
        return Err(NativeError::Silent);
    }
    Ok(())
}

fn hooks() -> Result<Rc<dyn HookApi>, NativeError> {
    crate::SHARED
        .with(|s| s.borrow().clone())
        .ok_or_else(|| NativeError::throw("Error", "the post type registry is not available"))
}

fn ports() -> PostTypePorts<'static> {
    PostTypePorts {
        translator: &PhpTranslator,
        rewrite: &PhpRewriteContext,
        titles: &PhpTitleSanitizer,
    }
}

/// A hook name from bytes (a post type name may be any bytes a `sanitize_key` callback returns).
fn hook_name(bytes: Vec<u8>) -> HookName {
    match String::from_utf8(bytes) {
        Ok(s) => HookName::from(HookValue::Str(s)),
        Err(e) => HookName::from(HookValue::Bytes(e.into_bytes())),
    }
}

/// `apply_filters( $hook, $value, ...$args )` on PHP values, through the fast path when nothing listens.
fn filter(hooks: &dyn HookApi, hook: &HookName, value: Value, args: Vec<Value>) -> R {
    if hooks
        .skip_if_empty(hook, Count::Filters)
        .map_err(hook_error)?
    {
        return Ok(value);
    }
    hooks.apply_filters(hook, value, args).map_err(hook_error)
}

/// `do_action( $hook, ...$args )`.
fn action(hooks: &dyn HookApi, hook: &HookName, args: Vec<Value>) -> Result<(), NativeError> {
    hooks.do_action(hook, args).map_err(hook_error)
}

fn php_in(value: &Value) -> Result<PhpValue, NativeError> {
    from_php(value).map_err(thrown)
}

fn php_out(value: &PhpValue) -> R {
    to_php(value).map_err(thrown)
}

/// `(object) $properties`.
fn std_object(properties: Vec<(PhpKey, PhpValue)>) -> R {
    php_out(&PhpValue::Object {
        class: "stdClass".to_owned(),
        properties,
    })
}

fn is_scalar(value: &Value) -> bool {
    matches!(
        &*value.read(),
        Value::Bool(_) | Value::Int(_) | Value::Float(_) | Value::Str(_) | Value::Bytes(_)
    )
}

fn is_string(value: &Value) -> bool {
    matches!(&*value.read(), Value::Str(_) | Value::Bytes(_))
}

/// `(string) $value` of a scalar.
fn text_bytes(value: &Value) -> Result<Vec<u8>, NativeError> {
    Ok(to_php_string(&php_in(value)?))
}

/// `__( $text )`, the text itself when l10n.php is not loaded.
fn translate(text: &str) -> R {
    if !exists("__") {
        return Ok(Value::Str(text.into()));
    }
    php("__", &[Value::Str(text.into())]).map_err(thrown)
}

/// `new WP_Error( $code, __( $message ) )`.
fn wp_error(code: &str, message: &str) -> R {
    let message = translate(message)?;
    helper("error", &[Value::Str(code.into()), message]).map_err(thrown)
}

/// `_doing_it_wrong( $function, __( $message ), $version )`, when WordPress has it.
fn doing_it_wrong(function: &str, message: &str, version: &str) -> Result<(), NativeError> {
    if !exists("_doing_it_wrong") {
        return Ok(());
    }
    let message = translate(message)?;
    php(
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

/// `sanitize_key( $key )`: WordPress's, with its filter; the unfiltered rule when formatting.php is not loaded.
fn sanitize_key(key: &Value) -> Result<Vec<u8>, NativeError> {
    if exists("sanitize_key") {
        let out = php("sanitize_key", std::slice::from_ref(key)).map_err(thrown)?;
        return text_bytes(&out);
    }
    if !is_scalar(key) {
        return Ok(Vec::new());
    }
    Ok(speedyspec_wp_posts::sanitize_key(&text_bytes(key)?).into_bytes())
}

/// `wp_parse_args( $args )` as an array of the rules' values.
fn parse_args(args: &Value) -> Result<Value, NativeError> {
    php("wp_parse_args", std::slice::from_ref(args)).map_err(thrown)
}

/// `get_post_type_object( $post_type )`: `null` unless it is a scalar naming a non-`empty()` entry of
/// `$wp_post_types`. The entry and its key's text.
fn type_object(post_type: &Value) -> Option<(String, Value)> {
    if !is_scalar(post_type) {
        return None;
    }
    let key = key_bytes(post_type)?;
    global_entry("wp_post_types", &key)
        .filter(truthy)
        .map(|v| (String::from_utf8_lossy(&key).into_owned(), v))
}

/// `get_post_status_object( $post_status )`: `null` unless it is a string naming a non-`empty()` entry of
/// `$wp_post_statuses`.
fn status_object(post_status: &Value) -> Option<(String, Value)> {
    if !is_string(post_status) {
        return None;
    }
    let key = key_bytes(post_status)?;
    global_entry("wp_post_statuses", &key)
        .filter(truthy)
        .map(|v| (String::from_utf8_lossy(&key).into_owned(), v))
}

/// `$object->$name` read from a PHP object, `null` when it has no such property.
fn object_property(object: &Value, name: &str) -> R {
    if !is_object(object) {
        return Ok(Value::Null);
    }
    let vars = object_vars(object).map_err(thrown)?;
    Ok(property(&vars, name).cloned().unwrap_or(Value::Null))
}

fn register_post_type(x: &[Value]) -> R {
    let args = x.get(1).cloned().unwrap_or(Value::Array(Vec::new()));
    helper("types_init", &[]).map_err(thrown)?;
    let name = sanitize_key(&a(x, 0))?;
    if name.is_empty() || name == b"0" || name.len() > 20 {
        doing_it_wrong("register_post_type", NAME_LENGTH, NAME_LENGTH_VERSION)?;
        return wp_error("post_type_length_invalid", NAME_LENGTH);
    }
    let name_value = string(&name);
    let hooks = hooks()?;

    // new WP_Post_Type( $post_type, $args ): set_props()' two args filters, then the rules.
    let args = parse_args(&args)?;
    let args = filter(
        &*hooks,
        &HookName::from("register_post_type_args"),
        args,
        vec![name_value.clone()],
    )?;
    let mut hook = b"register_".to_vec();
    hook.extend_from_slice(&name);
    hook.extend_from_slice(b"_post_type_args");
    let args = filter(&*hooks, &hook_name(hook), args, vec![name_value.clone()])?;
    let args = to_array(php_in(&args)?);
    let type_name = PostTypeName::new(String::from_utf8_lossy(&name));
    let resolved = resolve_post_type(type_name, args, &ports()).map_err(crate::read::post_error)?;
    stop_if_parked()?;
    // get_post_type_capabilities() remembers the meta capabilities before the labels are filtered.
    meta_caps_add(&resolved.meta_caps)?;
    let mut post_type = resolved.post_type;

    // get_post_type_labels()' filter.
    let mut hook = b"post_type_labels_".to_vec();
    hook.extend_from_slice(&name);
    let hook = hook_name(hook);
    if !hooks
        .skip_if_empty(&hook, Count::Filters)
        .map_err(hook_error)?
    {
        let labels = php_out(post_type.get("labels").unwrap_or(&PhpValue::Null))?;
        let filtered = hooks
            .apply_filters(&hook, labels, Vec::new())
            .map_err(hook_error)?;
        apply_filtered_labels(&mut post_type, to_array(php_in(&filtered)?));
    }

    // $post_type_object->add_supports().
    for call in add_supports(&mut post_type, &PhpPostTypeSupports) {
        stop_if_parked()?;
        let features = list(call.features.iter().map(|f| Value::Str(f.clone())));
        let args = php_out(&PhpValue::Array(
            call.args
                .iter()
                .enumerate()
                .map(|(i, v)| (PhpKey::Int(i as i64), v.clone()))
                .collect(),
        ))?;
        add_support(&string(call.post_type.as_bytes()), features, args)?;
    }
    stop_if_parked()?;

    let properties = php_out(&PhpValue::Array(post_type.properties().to_vec()))?;
    let object = helper("post_type_object", &[properties]).map_err(thrown)?;
    method(&object, "add_rewrite_rules", &[]).map_err(thrown)?;
    method(&object, "register_meta_boxes", &[]).map_err(thrown)?;
    helper("store_post_type", &[name_value.clone(), object.clone()]).map_err(thrown)?;
    method(&object, "add_hooks", &[]).map_err(thrown)?;
    method(&object, "register_taxonomies", &[]).map_err(thrown)?;

    let both = vec![name_value.clone(), object.clone()];
    action(
        &*hooks,
        &HookName::from("registered_post_type"),
        both.clone(),
    )?;
    let mut hook = b"registered_post_type_".to_vec();
    hook.extend_from_slice(&name);
    action(&*hooks, &hook_name(hook), both)?;
    Ok(object)
}

/// `$post_type_meta_caps[ $custom ] = $core` for each pair.
fn meta_caps_add(caps: &[(String, &'static str)]) -> Result<(), NativeError> {
    if caps.is_empty() {
        return Ok(());
    }
    helper("meta_caps_add", &[meta_caps_list(caps)]).map_err(thrown)?;
    Ok(())
}

fn meta_caps_list(caps: &[(String, &'static str)]) -> Value {
    Value::Array(
        caps.iter()
            .enumerate()
            .map(|(i, (custom, core))| {
                (
                    Key::Int(i as i64),
                    Value::Array(vec![
                        (Key::Int(0), Value::Str(custom.clone())),
                        (Key::Int(1), Value::Str((*core).to_owned())),
                    ]),
                )
            })
            .collect(),
    )
}

/// `add_post_type_support( $post_type, $features, ...$args )` on the global: `$features` is cast with `(array)` in
/// PHP, and `$args` is the list of PHP values to store (none stores `true`).
fn add_support(post_type: &Value, features: Value, args: Value) -> Result<(), NativeError> {
    helper("add_support", &[post_type.clone(), features, args]).map_err(thrown)?;
    Ok(())
}

/// A list of PHP values.
fn list(values: impl IntoIterator<Item = Value>) -> Value {
    Value::Array(
        values
            .into_iter()
            .enumerate()
            .map(|(i, v)| (Key::Int(i as i64), v))
            .collect(),
    )
}

/// `$post_type_meta_caps` as `unregister_post_type()` rebuilds it after the unset: from the `cap` of every type
/// left in the live `$wp_post_types` whose `map_meta_cap` is truthy, in order, each custom capability keeping its
/// first position and its last core capability.
fn rebuilt_meta_caps() -> Result<Vec<(String, &'static str)>, NativeError> {
    let mut caps: Vec<(String, &'static str)> = Vec::new();
    for remaining in speedyspec_wp_posts_contracts::PostTypes::post_types(&PhpPostTypes) {
        if remaining.get("map_meta_cap").is_none_or(is_empty) {
            continue;
        }
        let cap = match remaining.get("cap") {
            Some(PhpValue::Object { properties, .. }) => properties.as_slice(),
            Some(PhpValue::Array(items)) => items.as_slice(),
            _ => &[],
        };
        for (custom, core) in meta_capabilities(cap) {
            match caps.iter_mut().find(|(c, _)| *c == custom) {
                Some(entry) => entry.1 = core,
                None => caps.push((custom, core)),
            }
        }
    }
    stop_if_parked()?;
    Ok(caps)
}

fn unregister_post_type(x: &[Value]) -> R {
    let post_type = a(x, 0);
    let Some((key, object)) = type_object(&post_type) else {
        return wp_error("invalid_post_type", "Invalid post type.");
    };
    // The rules key the object by its `name`, as `remove_supports()` reads `$this->name`.
    let name = match object_property(&object, "name")? {
        v if is_string(&v) => String::from_utf8_lossy(&text_bytes(&v)?).into_owned(),
        _ => key,
    };
    let registered = post_type_from(&name, &object);
    stop_if_parked()?;
    let removal = match unregistration(registered, &PhpPostTypes) {
        Ok(removal) => removal,
        Err(PostTypeError::Builtin) => {
            return wp_error(
                "invalid_post_type",
                "Unregistering a built-in post type is not allowed",
            );
        }
        Err(_) => return wp_error("invalid_post_type", "Invalid post type."),
    };
    stop_if_parked()?;

    helper(
        "remove_supports",
        &[string(removal.supports_to_remove.as_bytes())],
    )
    .map_err(thrown)?;
    for step in [
        "remove_rewrite_rules",
        "unregister_meta_boxes",
        "remove_hooks",
        "unregister_taxonomies",
    ] {
        method(&object, step, &[]).map_err(thrown)?;
    }
    helper("forget_post_type", std::slice::from_ref(&post_type)).map_err(thrown)?;
    // WordPress 7.2 rebuilds the map after the unset, from the types left then.
    let meta_caps = rebuilt_meta_caps()?;
    helper("meta_caps_set", &[meta_caps_list(&meta_caps)]).map_err(thrown)?;
    action(
        &*hooks()?,
        &HookName::from("unregistered_post_type"),
        vec![post_type],
    )?;
    Ok(Value::Bool(true))
}

fn register_post_status(x: &[Value]) -> R {
    let args = x.get(1).cloned().unwrap_or(Value::Array(Vec::new()));
    helper("statuses_init", &[]).map_err(thrown)?;
    let args = to_array(php_in(&parse_args(&args)?)?);
    let name = sanitize_key(&a(x, 0))?;
    let status = resolve_post_status(PostStatusName::new(String::from_utf8_lossy(&name)), args);
    let properties = php_out(&PhpValue::Array(status.properties().to_vec()))?;
    helper("store_status", &[string(&name), properties]).map_err(thrown)
}

fn get_post_type_object(x: &[Value]) -> R {
    Ok(type_object(&a(x, 0)).map_or(Value::Null, |(_, v)| v))
}

fn get_post_status_object(x: &[Value]) -> R {
    Ok(status_object(&a(x, 0)).map_or(Value::Null, |(_, v)| v))
}

fn post_type_exists(x: &[Value]) -> R {
    Ok(Value::Bool(type_object(&a(x, 0)).is_some()))
}

/// An entry of a list `wp_filter_object_list()` filters: the fields the arguments name, read once.
struct Entry {
    value: Value,
    /// `(field, value)`: an object's set properties, or an array's entries, for the argument keys.
    fields: Vec<(Vec<u8>, PhpValue)>,
}

impl ListItem for Entry {
    fn field(&self, key: &PhpKey) -> Option<&PhpValue> {
        let wanted = key_to_string(key).into_bytes();
        self.fields
            .iter()
            .find(|(f, _)| *f == wanted)
            .map(|(_, v)| v)
    }
}

fn key_of(key: &Key) -> PhpKey {
    match key {
        Key::Int(i) => PhpKey::Int(*i),
        Key::Str(s) => PhpKey::Str(s.as_bytes().to_vec()),
    }
}

fn php_key(key: &PhpKey) -> Key {
    match key {
        PhpKey::Int(i) => Key::Int(*i),
        PhpKey::Str(s) => Key::Str(String::from_utf8_lossy(s).into_owned()),
    }
}

/// An object's properties or an array's entries, as PHP values; nothing for anything else.
fn raw_items(value: &Value) -> Result<(Vec<(Key, Value)>, bool), NativeError> {
    if is_object(value) {
        return Ok((object_vars(value).map_err(thrown)?, true));
    }
    Ok(match value.read().into_owned() {
        Value::Array(items) => (items, false),
        _ => (Vec::new(), false),
    })
}

/// The fields of `value` that `wanted` names, as PHP values: `isset( $obj->{$key} )` for an object,
/// `array_key_exists()` for an array, none for anything else.
fn raw_fields(value: &Value, wanted: &[Vec<u8>]) -> Result<Vec<(Vec<u8>, Value)>, NativeError> {
    let (items, object) = raw_items(value)?;
    Ok(items
        .into_iter()
        .map(|(k, v)| (crate::adapters::key_text(&k).into_bytes(), v))
        .filter(|(name, v)| wanted.contains(name) && !(object && matches!(&*v.read(), Value::Null)))
        .collect())
}

/// Whether a converted value holds an object the codec keeps live, which the rules cannot compare as PHP does.
fn holds_live(value: &PhpValue) -> bool {
    match value {
        PhpValue::Custom { .. } => true,
        PhpValue::Array(items)
        | PhpValue::Object {
            properties: items, ..
        } => items.iter().any(|(_, v)| holds_live(v)),
        _ => false,
    }
}

/// What an argument compared with PHP's `==` is replaced by, and what an entry's field that PHP found equal reads
/// as: the rules' `==` then matches exactly those entries.
const MATCHED: &[u8] = b"\0SpeedySpec\\WpPosts\\Matched";

/// `wp_filter_object_list( $GLOBALS[ $global ], $args, $operator, 'names' === $output ? 'name' : false )`.
///
/// `WP_List_Util::filter()` compares with `==`. The rules do that for scalars, arrays and `stdClass` copies; an
/// argument key where the argument or an entry's field holds any other object is compared by PHP itself.
fn filter_global(global: &str, x: &[Value]) -> R {
    let Some(list) = speedyspec_php::global(global).map(|v| v.read().into_owned()) else {
        return Ok(Value::Array(Vec::new()));
    };
    let Value::Array(list) = list else {
        return Ok(Value::Array(Vec::new()));
    };
    let raw_args = x.first().cloned().unwrap_or(Value::Array(Vec::new()));
    let mut args = to_array(php_in(&raw_args)?);
    let names = matches!(&*a_or(x, 1, "names").read(), Value::Str(s) if s == "names");
    let operator = String::from_utf8_lossy(&text_bytes(&a_or(x, 2, "and"))?).into_owned();
    let wanted: Vec<Vec<u8>> = args
        .iter()
        .map(|(k, _)| key_to_string(k).into_bytes())
        .collect();
    let filtering = !args.is_empty() && FilterOperator::parse(&operator).is_some();
    let mut entries = Vec::with_capacity(list.len());
    for (key, value) in list {
        let raw = if filtering {
            raw_fields(&value, &wanted)?
        } else {
            Vec::new()
        };
        let fields = raw
            .iter()
            .map(|(name, v)| Ok((name.clone(), php_in(v)?)))
            .collect::<Result<Vec<_>, NativeError>>()?;
        entries.push((key, value, raw, fields));
    }
    // The argument keys PHP must compare.
    let raw_args = raw_items(&raw_args)?.0;
    for (i, name) in wanted.iter().enumerate() {
        let involved = holds_live(&args[i].1)
            || entries
                .iter()
                .any(|(_, _, _, fields)| fields.iter().any(|(f, v)| f == name && holds_live(v)));
        if !involved {
            continue;
        }
        let Some(arg) = raw_args
            .iter()
            .find(|(k, _)| crate::adapters::key_text(k).as_bytes() == name.as_slice())
            .map(|(_, v)| v.clone())
        else {
            continue;
        };
        args[i].1 = PhpValue::Str(MATCHED.to_vec());
        for (_, _, raw, fields) in &mut entries {
            let Some(field) = raw.iter().find(|(f, _)| f == name).map(|(_, v)| v.clone()) else {
                continue;
            };
            let equal = truthy(&helper("loose_equals", &[arg.clone(), field]).map_err(thrown)?);
            fields.retain(|(f, _)| f != name);
            if equal {
                fields.push((name.clone(), PhpValue::Str(MATCHED.to_vec())));
            }
        }
    }
    let items = entries
        .into_iter()
        .map(|(key, value, _, fields)| (key_of(&key), Entry { value, fields }))
        .collect();
    let kept = filter_object_list(items, &args, &operator);
    if !names {
        return Ok(Value::Array(
            kept.into_iter()
                .map(|(k, e)| (php_key(&k), e.value))
                .collect(),
        ));
    }
    // WP_List_Util::pluck( 'name' ).
    let mut out = Vec::with_capacity(kept.len());
    for (key, entry) in kept {
        let name = if is_object(&entry.value) {
            object_property(&entry.value, "name")?
        } else if let Value::Array(items) = entry.value.read().into_owned() {
            items
                .into_iter()
                .find(|(k, _)| matches!(k, Key::Str(s) if s == "name"))
                .map_or(Value::Null, |(_, v)| v)
        } else {
            doing_it_wrong(
                "WP_List_Util::pluck",
                "Values for the input array must be either objects or arrays.",
                "6.2.0",
            )?;
            continue;
        };
        out.push((php_key(&key), name));
    }
    Ok(Value::Array(out))
}

/// The `i`th argument, or its default when it was not passed.
fn a_or(args: &[Value], i: usize, default: &str) -> Value {
    args.get(i)
        .cloned()
        .unwrap_or_else(|| Value::Str(default.into()))
}

fn get_post_types(x: &[Value]) -> R {
    filter_global("wp_post_types", x)
}

fn get_post_stati(x: &[Value]) -> R {
    filter_global("wp_post_statuses", x)
}

/// `$value` as an array key's bytes, `null` as `""`; `None` for arrays and objects.
fn feature_key(value: &Value) -> Option<Vec<u8>> {
    if matches!(&*value.read(), Value::Null) {
        return Some(Vec::new());
    }
    key_bytes(value)
}

fn post_type_supports(x: &[Value]) -> R {
    let (Some(post_type), Some(feature)) = (feature_key(&a(x, 0)), feature_key(&a(x, 1))) else {
        return Ok(Value::Bool(false));
    };
    Ok(Value::Bool(
        speedyspec_wp_posts_contracts::PostTypeSupports::supports(
            &PhpPostTypeSupports,
            &String::from_utf8_lossy(&post_type),
            &String::from_utf8_lossy(&feature),
        ),
    ))
}

fn add_post_type_support(x: &[Value]) -> R {
    // The args are stored as PHP gave them, so an object keeps its identity; `(array) $feature` is PHP's, so a
    // feature that is no array key throws as it does in WordPress.
    let args = list(x.get(2..).unwrap_or_default().iter().cloned());
    add_support(&a(x, 0), a(x, 1), args)?;
    Ok(Value::Null)
}

fn remove_post_type_support(x: &[Value]) -> R {
    helper("remove_support", &[a(x, 0), a(x, 1)]).map_err(thrown)?;
    Ok(Value::Null)
}

fn get_all_post_type_supports(x: &[Value]) -> R {
    let entry = feature_key(&a(x, 0)).and_then(|k| global_entry("_wp_post_type_features", &k));
    Ok(match entry {
        Some(v) if !matches!(&*v.read(), Value::Null) => v,
        _ => Value::Array(Vec::new()),
    })
}

fn get_post_types_by_support(x: &[Value]) -> R {
    // `array_fill_keys( (array) $feature, true )`: each value becomes a string, then an array key.
    let features: Vec<String> = to_array(php_in(&a(x, 0))?)
        .iter()
        .map(|(_, f)| String::from_utf8_lossy(&to_php_string(f)).into_owned())
        .collect();
    let features: Vec<&str> = features.iter().map(String::as_str).collect();
    let operator = String::from_utf8_lossy(&text_bytes(&a_or(x, 1, "and"))?).into_owned();
    let found = post_types_by_support(&PhpPostTypeSupports, &features, &operator);
    stop_if_parked()?;
    Ok(Value::Array(
        found
            .iter()
            .enumerate()
            .map(|(i, k)| {
                let v = match k {
                    PhpKey::Int(n) => Value::Int(*n),
                    PhpKey::Str(s) => string(s),
                };
                (Key::Int(i as i64), v)
            })
            .collect(),
    ))
}

fn is_post_type_hierarchical(x: &[Value]) -> R {
    // The raw property, as WordPress returns it.
    match type_object(&a(x, 0)) {
        Some((_, object)) => object_property(&object, "hierarchical"),
        None => Ok(Value::Bool(false)),
    }
}

/// `true === apply_filters( $hook, $is_viewable, $object )`.
fn viewable_filter(hook: &str, is_viewable: bool, object: Value) -> R {
    let out = filter(
        &*hooks()?,
        &HookName::from(hook),
        Value::Bool(is_viewable),
        vec![object],
    )?;
    Ok(Value::Bool(matches!(&*out.read(), Value::Bool(true))))
}

fn is_post_type_viewable_native(x: &[Value]) -> R {
    let mut object = a(x, 0);
    if is_scalar(&object) {
        match type_object(&object) {
            Some((_, found)) => object = found,
            None => return Ok(Value::Bool(false)),
        }
    }
    if !is_object(&object) {
        return Ok(Value::Bool(false));
    }
    let post_type = post_type_from("", &object);
    stop_if_parked()?;
    viewable_filter(
        "is_post_type_viewable",
        is_post_type_viewable(&post_type),
        object,
    )
}

fn is_post_status_viewable_native(x: &[Value]) -> R {
    let mut object = a(x, 0);
    if is_scalar(&object) {
        match status_object(&object) {
            Some((_, found)) => object = found,
            None => return Ok(Value::Bool(false)),
        }
    }
    if !is_object(&object) {
        return Ok(Value::Bool(false));
    }
    let status = post_status_from("", &object);
    stop_if_parked()?;
    let set = |prop| status.get(prop).is_some_and(|v| !is_empty(v));
    if set("internal") || set("protected") {
        return Ok(Value::Bool(false));
    }
    viewable_filter(
        "is_post_status_viewable",
        is_post_status_viewable(&status),
        object,
    )
}

/// What the label functions read from a post type (or taxonomy) object.
struct LabelObject {
    name: String,
    label: Option<PhpValue>,
    labels: PhpValue,
    hierarchical: bool,
}

impl LabelObject {
    fn read(object: &Value) -> Result<LabelObject, NativeError> {
        let properties = if is_object(object) {
            post_type_from("", object).properties().to_vec()
        } else {
            Vec::new()
        };
        stop_if_parked()?;
        let prop = |name| get(&properties, name).cloned();
        Ok(LabelObject {
            name: String::from_utf8_lossy(&to_php_string(&prop("name").unwrap_or(PhpValue::Null)))
                .into_owned(),
            label: prop("label").filter(|v| !matches!(v, PhpValue::Null)),
            labels: prop("labels").unwrap_or(PhpValue::Null),
            hierarchical: prop("hierarchical").is_some_and(|v| !is_empty(&v)),
        })
    }

    fn source(&self) -> LabelSource<'_> {
        LabelSource {
            name: &self.name,
            label: self.label.as_ref(),
            labels: &self.labels,
            hierarchical: self.hierarchical,
        }
    }
}

/// `$data_object->labels = (object) $data_object->labels`, after `_get_custom_object_labels()`' fill-ins.
fn store_given_labels(object: &Value, given: Vec<(PhpKey, PhpValue)>) -> Result<(), NativeError> {
    if !is_object(object) {
        return Ok(());
    }
    let labels = std_object(given)?;
    helper(
        "set_prop",
        &[object.clone(), Value::Str("labels".into()), labels],
    )
    .map_err(thrown)?;
    Ok(())
}

fn get_post_type_labels(x: &[Value]) -> R {
    let object = a(x, 0);
    let read = LabelObject::read(&object)?;
    let given = custom_object_labels(&read.source(), &[]).given;
    let labels =
        post_type_labels(&read.source(), &PhpTranslator).map_err(crate::read::post_error)?;
    stop_if_parked()?;
    store_given_labels(&object, given)?;
    let hook = hook_name(format!("post_type_labels_{}", read.name).into_bytes());
    let hooks = hooks()?;
    if hooks
        .skip_if_empty(&hook, Count::Filters)
        .map_err(hook_error)?
    {
        return std_object(labels);
    }
    let filtered = hooks
        .apply_filters(&hook, std_object(labels.clone())?, Vec::new())
        .map_err(hook_error)?;
    std_object(merge_filtered_labels(labels, to_array(php_in(&filtered)?)))
}

fn get_custom_object_labels(x: &[Value]) -> R {
    let object = a(x, 0);
    let read = LabelObject::read(&object)?;
    // `$nohier_vs_hier_defaults`: `key => [ non-hierarchical, hierarchical ]`.
    let defaults: Vec<(String, LabelPair)> = to_array(php_in(&a(x, 1))?)
        .into_iter()
        .map(|(key, pair)| {
            let pair = to_array(pair);
            let side = |i: i64| {
                get_index(&pair, i).and_then(|v| match v {
                    PhpValue::Null => None,
                    other => Some(String::from_utf8_lossy(&to_php_string(other)).into_owned()),
                })
            };
            (key_to_string(&key), [side(0), side(1)])
        })
        .collect();
    let defaults: Vec<(&str, LabelPair)> = defaults
        .iter()
        .map(|(k, pair)| (k.as_str(), pair.clone()))
        .collect();
    let custom = custom_object_labels(&read.source(), &defaults);
    store_given_labels(&object, custom.given)?;
    std_object(custom.labels)
}

fn get_index(items: &[(PhpKey, PhpValue)], index: i64) -> Option<&PhpValue> {
    items
        .iter()
        .find(|(k, _)| *k == PhpKey::Int(index))
        .map(|(_, v)| v)
}

fn get_post_type_capabilities(x: &[Value]) -> R {
    let object = a(x, 0);
    let vars = if is_object(&object) {
        object_vars(&object).map_err(thrown)?
    } else {
        Vec::new()
    };
    let raw = |name| property(&vars, name).cloned().unwrap_or(Value::Null);
    let capability_type = raw("capability_type");
    let read_type = php_in(&capability_type)?;
    if !matches!(read_type, PhpValue::Array(_)) && is_object(&object) {
        // `$args->capability_type = array( $args->capability_type, $args->capability_type . 's' )`.
        let mut plural = to_php_string(&read_type);
        plural.push(b's');
        let pair = Value::Array(vec![
            (Key::Int(0), capability_type.clone()),
            (Key::Int(1), string(&plural)),
        ]);
        helper(
            "set_prop",
            &[object.clone(), Value::Str("capability_type".into()), pair],
        )
        .map_err(thrown)?;
    }
    let map_meta_cap = truthy(&raw("map_meta_cap"));
    let caps = post_type_capabilities(&read_type, map_meta_cap, &php_in(&raw("capabilities"))?);
    if map_meta_cap {
        meta_caps_add(&meta_capabilities(&caps))?;
    }
    std_object(caps)
}

fn create_initial_post_types(_: &[Value]) -> R {
    php("WP_Post_Type::reset_default_labels", &[]).map_err(thrown)?;
    let attachment_pages_enabled = exists("get_option")
        && matches!(
            &*php("get_option", &[Value::Str("wp_attachment_pages_enabled".into())])
                .map_err(thrown)?
                .read(),
            Value::Str(s) if s == "1"
        );
    let calls = initial_calls(
        &InitialOptions {
            attachment_pages_enabled,
        },
        &PhpTranslator,
    )
    .map_err(crate::read::post_error)?;
    stop_if_parked()?;
    for call in calls {
        match call {
            InitialCall::PostType(name, args) => {
                register_post_type(&[Value::Str(name.into()), php_out(&PhpValue::Array(args))?])?;
            }
            InitialCall::AddSupport(post_type, feature) => {
                add_post_type_support(&[Value::Str(post_type.into()), Value::Str(feature.into())])?;
            }
            InitialCall::RemoveSupport(post_type, feature) => {
                remove_post_type_support(&[
                    Value::Str(post_type.into()),
                    Value::Str(feature.into()),
                ])?;
            }
            InitialCall::Status(name, args) => {
                register_post_status(&[Value::Str(name.into()), php_out(&PhpValue::Array(args))?])?;
            }
        }
    }
    Ok(Value::Null)
}

/// Clears what a call from Rust may have left, runs `f`, then rethrows the first failure the work met: the
/// callback's own exception, a port's parked one, or one `wpdb` hid behind an empty answer (`take_hook_error()`).
///
/// A native function PHP calls while another one runs (from a hook callback) keeps the outer call's parked
/// exception for it. Every call, nested or not, starts with `wpdb`'s side channel empty and takes it once `f`
/// returns, as meta-php's natives do, so a failure a nested call met (and a callback caught) is never rethrown by
/// the call around it.
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
        restore(outer);
    }
    let hidden = speedyspec_wp_db_contracts::take_hook_error();
    if DEPTH.with(|d| {
        d.set(d.get() - 1);
        d.get() == 0
    }) {
        // The markers only live for one outermost call: the objects they hold are released now, not at request end.
        crate::codec::clear_live();
    }
    match (result, parked, hidden) {
        (Err(NativeError::Rethrow(exception)), _, _) => Err(NativeError::Rethrow(exception)),
        // A port's exception stopped the work: it is what PHP sees.
        (_, Some(exception), _) => Err(NativeError::Rethrow(exception)),
        // `wpdb` answered false or 0 where a `query` callback threw: the exception still reaches PHP.
        (_, None, Some(hook)) => Err(hook_error(hook)),
        (other, None, None) => other,
    }
}

/// Forgets how deeply native functions were nested, at the end of a request (a callback that exits inside one
/// never returns to [`guarded`]).
pub(crate) fn reset_depth() {
    DEPTH.with(|d| d.set(0));
}

/// The functions the facade calls, with WordPress's names.
pub(crate) const ENTRIES: &[(&str, Native)] = &[
    ("register_post_type", register_post_type),
    ("unregister_post_type", unregister_post_type),
    ("register_post_status", register_post_status),
    ("get_post_type_object", get_post_type_object),
    ("get_post_types", get_post_types),
    ("get_post_stati", get_post_stati),
    ("get_post_status_object", get_post_status_object),
    ("post_type_exists", post_type_exists),
    ("post_type_supports", post_type_supports),
    ("add_post_type_support", add_post_type_support),
    ("remove_post_type_support", remove_post_type_support),
    ("get_all_post_type_supports", get_all_post_type_supports),
    ("get_post_types_by_support", get_post_types_by_support),
    ("is_post_type_hierarchical", is_post_type_hierarchical),
    ("is_post_type_viewable", is_post_type_viewable_native),
    ("is_post_status_viewable", is_post_status_viewable_native),
    ("get_post_type_labels", get_post_type_labels),
    ("get_post_type_capabilities", get_post_type_capabilities),
    ("_get_custom_object_labels", get_custom_object_labels),
    ("create_initial_post_types", create_initial_post_types),
];

pub(crate) fn functions() -> Vec<FunctionBuilder<'static>> {
    ENTRIES
        .iter()
        .chain(crate::read::ENTRIES)
        .map(|&(name, f)| {
            native::function(&format!("{NATIVE}{name}"), move |args: &[Value]| {
                guarded(f, args)
            })
        })
        .collect()
}
