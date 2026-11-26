//! The PHP adapters of the ports phases 3 and 4 add.

use speedyspec_php_ext::Value;
use speedyspec_wp_options::ExternalObjectCache;

use crate::adapters::{call, exists, truthy};

/// `wp_using_ext_object_cache()`: WordPress's tests toggle it in PHP, so PHP answers (false when it is undefined).
pub struct PhpExternalObjectCache;

impl ExternalObjectCache for PhpExternalObjectCache {
    fn using(&self) -> bool {
        exists("wp_using_ext_object_cache").unwrap_or(false)
            && call("wp_using_ext_object_cache", &[]).is_ok_and(|v| truthy(&v))
    }
}

/// `$value` as PHP holds it: a `Callback` carried opaquely becomes its PHP callable again.
pub(crate) fn plain(value: Value) -> Value {
    match value {
        Value::Opaque(any) => match any.downcast_ref::<speedyspec_wp_hooks_contracts::Callback>() {
            Some(cb) => cb.value(),
            None => Value::Opaque(any),
        },
        other => other,
    }
}
