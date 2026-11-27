//! The value helpers the filtered use cases share: PHP values to and from the Hook API's values, hook names from
//! bytes, and `apply_filters()` through the fast path when nothing listens.

use speedyspec_php_serialize::{PhpValue, from_hook_value, to_hook_value};
use speedyspec_wp_hooks_contracts::{Count, HookApi, HookError, HookName, HookValue};
use speedyspec_wp_posts_contracts::PostError;

/// A PHP value as the hooks carry it.
pub(crate) fn hv(value: &PhpValue) -> HookValue {
    to_hook_value(value)
}

/// A hook's value as a PHP value. A foreign opaque value (a live object with no serialized form) cannot be one.
pub(crate) fn pv(value: &HookValue) -> Result<PhpValue, PostError> {
    from_hook_value(value).map_err(|e| {
        PostError::Hook(HookError::message(format!(
            "a post hook returned a value the posts API cannot use: {e}"
        )))
    })
}

/// A byte string as the hooks carry it: text when it is UTF-8.
pub(crate) fn bytes_value(bytes: &[u8]) -> HookValue {
    match std::str::from_utf8(bytes) {
        Ok(s) => HookValue::Str(s.to_owned()),
        Err(_) => HookValue::Bytes(bytes.to_vec()),
    }
}

/// A hook name built from bytes (a field or a post type name may be any bytes).
pub(crate) fn byte_name(bytes: Vec<u8>) -> HookName {
    match String::from_utf8(bytes) {
        Ok(s) => HookName::from(HookValue::Str(s)),
        Err(e) => HookName::from(HookValue::Bytes(e.into_bytes())),
    }
}

/// `apply_filters( $hook, $value, ...$args )` on hook values, through the fast path when nothing listens: then the
/// value comes back as given and `args` is never built.
pub(crate) fn filter_hook(
    hooks: &dyn HookApi,
    hook: &HookName,
    value: HookValue,
    args: impl FnOnce() -> Vec<HookValue>,
) -> Result<HookValue, PostError> {
    if hooks.skip_if_empty(hook, Count::Filters)? {
        return Ok(value);
    }
    Ok(hooks.apply_filters(hook, value, args())?)
}

/// [`filter_hook`] on a PHP value: the value as the hooks carry it, and back. The fast path is checked here first so
/// that, with nothing listening, the value comes back as given without the two conversions.
pub(crate) fn filter(
    hooks: &dyn HookApi,
    hook: &HookName,
    value: PhpValue,
    args: impl FnOnce() -> Vec<HookValue>,
) -> Result<PhpValue, PostError> {
    if hooks.skip_if_empty(hook, Count::Filters)? {
        return Ok(value);
    }
    pv(&filter_hook(hooks, hook, hv(&value), args)?)
}
