//! The post type rules' ports over PHP (Adapter): the registries read from PHP's globals live, and the WordPress
//! functions the rules call (`__()`, `_x()`, `is_admin()`, `get_option()`, `sanitize_title_with_dashes()`).
//!
//! During a PHP request PHP's globals are authoritative (the spec's *Who owns registered data*): a plugin that
//! mutates `$wp_post_types['post']` after registering it, or replaces the entry, is seen by the next read, because
//! every read goes to the global.
//!
//! A port whose contract cannot fail (it answers `bool`, `String` or `Option`) parks the PHP exception it met
//! ([`park`]). From then on no more PHP runs through [`ask`], and the native function that is running stops and
//! rethrows what was parked (`registry::guarded`), as WordPress stops where the exception is thrown.

use std::cell::RefCell;

use speedyspec_php_ext::{Key, Value};
use speedyspec_php_serialize::semantics::{bytes_truthy, to_php_string};
use speedyspec_php_serialize::{PhpKey, PhpValue};
use speedyspec_wp_posts_contracts::{
    PostStatusName, PostStatusObject, PostStatuses, PostType, PostTypeName, PostTypeSupports,
    PostTypes, RewriteContext, TitleSanitizer, Translator,
};

use crate::codec::{Thrown, from_php, is_object, object_vars, php};

thread_local! {
    static PARKED: RefCell<Option<Value>> = const { RefCell::new(None) };
}

/// Keeps the first exception a port met while a native function runs.
pub(crate) fn park(thrown: Thrown) {
    if let Some(exception) = thrown.0 {
        PARKED.with(|p| {
            p.borrow_mut().get_or_insert(exception);
        });
    }
}

/// Puts back a failure an outer call parked (`registry::guarded`).
pub(crate) fn restore(exception: Value) {
    PARKED.with(|p| {
        p.borrow_mut().get_or_insert(exception);
    });
}

/// Whether a port has parked an exception the running native function has not rethrown yet.
pub(crate) fn is_parked() -> bool {
    PARKED.with(|p| p.borrow().is_some())
}

/// What a port parked, taken once.
pub(crate) fn take_parked() -> Option<Value> {
    PARKED.with(|p| p.borrow_mut().take())
}

/// Calls a PHP function; an exception is parked and the answer is `None`. Once something is parked no more PHP
/// runs: the answer is `None` at once, as WordPress would not have got here.
pub(crate) fn ask(name: &str, args: &[Value]) -> Option<Value> {
    if is_parked() {
        return None;
    }
    match php(name, args) {
        Ok(v) => Some(v),
        Err(thrown) => {
            park(thrown);
            None
        }
    }
}

/// `function_exists( $name )`.
pub(crate) fn exists(name: &str) -> bool {
    !is_parked()
        && matches!(
            php("function_exists", &[Value::Str(name.into())]),
            Ok(Value::Bool(true))
        )
}

/// PHP's truthiness of a PHP value.
pub(crate) fn truthy(value: &Value) -> bool {
    match &*value.read() {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Int(i) => *i != 0,
        Value::Float(f) => *f != 0.0,
        Value::Str(s) => bytes_truthy(s.as_bytes()),
        Value::Bytes(b) => bytes_truthy(b),
        Value::Array(items) => !items.is_empty(),
        Value::Opaque(_) => true,
    }
}

/// `$value` as an array key's bytes, PHP's conversion applied as `$array[ $value ]` does: `true` is `1`, `false`
/// `0`, a float is truncated. `None` for null (the caller's `is_scalar()` check refuses it) and for arrays and
/// objects, which are not keys.
pub(crate) fn key_bytes(value: &Value) -> Option<Vec<u8>> {
    match &*value.read() {
        Value::Bool(b) => Some(if *b { b"1".to_vec() } else { b"0".to_vec() }),
        Value::Int(i) => Some(i.to_string().into_bytes()),
        Value::Float(f) if f.is_finite() => Some((f.trunc() as i64).to_string().into_bytes()),
        Value::Str(s) => Some(s.as_bytes().to_vec()),
        Value::Bytes(b) => Some(b.clone()),
        _ => None,
    }
}

/// An array key's text: an int key in decimal.
pub(crate) fn key_text(key: &Key) -> String {
    match key {
        Key::Int(i) => i.to_string(),
        Key::Str(s) => s.clone(),
    }
}

/// `$GLOBALS[ $global ]`'s entries, references followed; empty when it is unset or not an array.
pub(crate) fn global_entries(global: &str) -> Vec<(Key, Value)> {
    match speedyspec_php::global(global).map(|v| v.read().into_owned()) {
        Some(Value::Array(items)) => items,
        _ => Vec::new(),
    }
}

/// `$GLOBALS[ $global ][ $key ]`, `None` when absent.
pub(crate) fn global_entry(global: &str, key: &[u8]) -> Option<Value> {
    speedyspec_php::global_array_get(global, key)
}

/// An object's properties as the rules read them; a PHP failure is parked and reads as none.
fn properties_of(object: &Value) -> Vec<(PhpKey, PhpValue)> {
    if is_parked() {
        return Vec::new();
    }
    let vars = match object_vars(object) {
        Ok(vars) => vars,
        Err(thrown) => {
            park(thrown);
            return Vec::new();
        }
    };
    let mut properties = Vec::with_capacity(vars.len());
    for (k, v) in vars {
        match from_php(&v) {
            Ok(value) => properties.push((PhpKey::Str(key_text(&k).into_bytes()), value)),
            Err(thrown) => {
                park(thrown);
                return Vec::new();
            }
        }
    }
    properties
}

/// A post type object as the rules read it, keyed by `key` (its `$wp_post_types` key).
pub(crate) fn post_type_from(key: &str, object: &Value) -> PostType {
    PostType::new(PostTypeName::new(key), properties_of(object))
}

/// A post status object as the rules read it, keyed by `key`.
pub(crate) fn post_status_from(key: &str, object: &Value) -> PostStatusObject {
    PostStatusObject::new(PostStatusName::new(key), properties_of(object))
}

/// `$wp_post_types`, read live (Registry). Only objects are post types: `get_post_type_object()` hands back
/// whatever is stored, but the rules read properties.
pub struct PhpPostTypes;

impl PostTypes for PhpPostTypes {
    fn post_type(&self, name: &str) -> Option<PostType> {
        global_entry("wp_post_types", name.as_bytes())
            .filter(|v| truthy(v) && is_object(v))
            .map(|v| post_type_from(name, &v))
    }

    fn post_types(&self) -> Vec<PostType> {
        global_entries("wp_post_types")
            .into_iter()
            .filter(|(_, v)| is_object(v))
            .map(|(k, v)| post_type_from(&key_text(&k), &v))
            .collect()
    }
}

/// `$wp_post_statuses`, read live (Registry).
pub struct PhpPostStatuses;

impl PostStatuses for PhpPostStatuses {
    fn status(&self, name: &str) -> Option<PostStatusObject> {
        global_entry("wp_post_statuses", name.as_bytes())
            .filter(|v| truthy(v) && is_object(v))
            .map(|v| post_status_from(name, &v))
    }

    fn statuses(&self) -> Vec<PostStatusObject> {
        global_entries("wp_post_statuses")
            .into_iter()
            .filter(|(_, v)| is_object(v))
            .map(|(k, v)| post_status_from(&key_text(&k), &v))
            .collect()
    }
}

/// `$_wp_post_type_features`, read live.
pub struct PhpPostTypeSupports;

impl PostTypeSupports for PhpPostTypeSupports {
    /// `isset( $_wp_post_type_features[ $post_type ][ $feature ] )`.
    fn supports(&self, post_type: &str, feature: &str) -> bool {
        let Some(features) = global_entry("_wp_post_type_features", post_type.as_bytes()) else {
            return false;
        };
        let read = features.read();
        let Value::Array(items) = &*read else {
            return false;
        };
        let wanted = canonical(feature.as_bytes());
        items
            .iter()
            .any(|(k, v)| *k == wanted && !matches!(&*v.read(), Value::Null))
    }

    fn features(&self) -> Vec<(String, Vec<(String, PhpValue)>)> {
        let mut out = Vec::new();
        for (post_type, features) in global_entries("_wp_post_type_features") {
            let read = features.read();
            let Value::Array(items) = &*read else {
                continue;
            };
            let mut list = Vec::with_capacity(items.len());
            for (feature, value) in items {
                match from_php(value) {
                    Ok(value) => list.push((key_text(feature), value)),
                    Err(thrown) => {
                        park(thrown);
                        return Vec::new();
                    }
                }
            }
            out.push((key_text(&post_type), list));
        }
        out
    }
}

/// `$bytes` as PHP keys an array by it: a canonical integer string is an int key.
pub(crate) fn canonical(bytes: &[u8]) -> Key {
    if speedyspec_php_serialize::semantics::is_integer_key(bytes)
        && let Some(i) = std::str::from_utf8(bytes).ok().and_then(|s| s.parse().ok())
    {
        return Key::Int(i);
    }
    Key::Str(String::from_utf8_lossy(bytes).into_owned())
}

/// A PHP answer as bytes (`(string)` of a scalar).
fn answer_bytes(value: &Value) -> Vec<u8> {
    match from_php(value) {
        Ok(v) => to_php_string(&v),
        Err(thrown) => {
            park(thrown);
            Vec::new()
        }
    }
}

/// `__()` and `_x()`; the text itself when l10n.php is not loaded.
pub struct PhpTranslator;

impl Translator for PhpTranslator {
    fn translate(&self, text: &str, context: Option<&str>) -> String {
        let (name, args) = match context {
            None => ("__", vec![Value::Str(text.into())]),
            Some(context) => (
                "_x",
                vec![Value::Str(text.into()), Value::Str(context.into())],
            ),
        };
        if !exists(name) {
            return text.to_owned();
        }
        match ask(name, &args) {
            Some(v) => String::from_utf8_lossy(&answer_bytes(&v)).into_owned(),
            None => text.to_owned(),
        }
    }
}

/// `is_admin() || get_option( 'permalink_structure' )`.
pub struct PhpRewriteContext;

impl RewriteContext for PhpRewriteContext {
    fn rewrite_applies(&self) -> bool {
        if exists("is_admin") && ask("is_admin", &[]).is_some_and(|v| truthy(&v)) {
            return true;
        }
        exists("get_option")
            && ask("get_option", &[Value::Str("permalink_structure".into())])
                .is_some_and(|v| truthy(&v))
    }
}

/// `sanitize_title_with_dashes( $title )`.
pub struct PhpTitleSanitizer;

impl TitleSanitizer for PhpTitleSanitizer {
    fn sanitize_title_with_dashes(&self, title: &[u8]) -> Vec<u8> {
        match ask("sanitize_title_with_dashes", &[crate::codec::string(title)]) {
            Some(v) => answer_bytes(&v),
            None => Vec::new(),
        }
    }
}
