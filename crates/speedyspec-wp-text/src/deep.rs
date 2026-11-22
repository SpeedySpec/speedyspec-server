//! `map_deep()` and the `*_deep()` helpers over PHP-shaped values.
//!
//! Objects are opaque here and come back unchanged; the PHP surface walks their properties, as PHP's `map_deep()`
//! does.

use speedyspec_hooks::Value as HookValue;

use crate::strings::{bytes_value, rawurlencode, to_php_string, urldecode, urlencode};

/// WordPress's `map_deep()`: `f` on every scalar, keeping the arrays' keys and order.
///
/// @param value
///     The value.
/// @param f
///     The callback, given each scalar (and `null`).
/// @return
///     The mapped value.
pub fn map_deep(value: &HookValue, f: &mut dyn FnMut(&HookValue) -> HookValue) -> HookValue {
    match value {
        HookValue::Array(items) => HookValue::Array(items.iter().map(|(k, v)| (k.clone(), map_deep(v, f))).collect()),
        HookValue::Opaque(_) => value.clone(),
        scalar => f(scalar),
    }
}

/// A string function on a scalar, as PHP calls it: the scalar becomes a string first.
fn on_string(value: &HookValue, f: fn(&[u8]) -> Vec<u8>) -> HookValue {
    match to_php_string(value) {
        Some(text) => bytes_value(f(&text)),
        None => value.clone(),
    }
}

/// WordPress's `stripslashes_from_strings_only()`.
///
/// @param value
///     The value.
/// @return
///     A string with `stripslashes()` applied; anything else as it is.
pub fn stripslashes_from_strings_only(value: &HookValue) -> HookValue {
    match value {
        HookValue::Str(s) => bytes_value(speedyspec_wp_text_escape::stripslashes(s.as_bytes()).into_owned()),
        HookValue::Bytes(b) => bytes_value(speedyspec_wp_text_escape::stripslashes(b).into_owned()),
        other => other.clone(),
    }
}

/// WordPress's `stripslashes_deep()`.
///
/// @param value
///     The value.
/// @return
///     Every string in it unslashed.
pub fn stripslashes_deep(value: &HookValue) -> HookValue {
    map_deep(value, &mut stripslashes_from_strings_only)
}

/// WordPress's `urlencode_deep()`.
///
/// @param value
///     The value.
/// @return
///     Every scalar in it `urlencode()`d (so as a string).
pub fn urlencode_deep(value: &HookValue) -> HookValue {
    map_deep(value, &mut |v| on_string(v, urlencode))
}

/// WordPress's `rawurlencode_deep()`.
///
/// @param value
///     The value.
/// @return
///     Every scalar in it `rawurlencode()`d.
pub fn rawurlencode_deep(value: &HookValue) -> HookValue {
    map_deep(value, &mut |v| on_string(v, rawurlencode))
}

/// WordPress's `urldecode_deep()`.
///
/// @param value
///     The value.
/// @return
///     Every scalar in it `urldecode()`d.
pub fn urldecode_deep(value: &HookValue) -> HookValue {
    map_deep(value, &mut |v| on_string(v, urldecode))
}
