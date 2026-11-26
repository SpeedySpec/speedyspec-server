//! `sanitize_meta()` and `is_protected_meta()` with its filter.

use std::cell::RefCell;
use std::rc::Rc;

use speedyspec_php_serialize::PhpValue;
use speedyspec_wp_hooks_contracts::{HasFilterResult, HookApi, HookError, HookName, HookValue};
use speedyspec_wp_meta_contracts::{MetaKey, MetadataError, ProtectedMeta};

use crate::MetaHookNames;
use crate::api::{byte_name, filter, hv, key_value, pv};

/// `sanitize_meta( $meta_key, $meta_value, $object_type, $object_subtype )`.
///
/// With a non-empty subtype and a callback on `sanitize_{type}_meta_{key}_for_{subtype}`, that filter alone runs
/// (`$meta_value, $meta_key, $object_type, $object_subtype`). Otherwise `sanitize_{type}_meta_{key}` runs
/// (`$meta_value, $meta_key, $object_type`).
pub fn sanitize_meta(
    hooks: &dyn HookApi,
    key: &MetaKey,
    value: PhpValue,
    object_type: &str,
    subtype: &str,
) -> Result<PhpValue, MetadataError> {
    let names = MetaHookNames::for_object_type(object_type);
    if !subtype.is_empty() && subtype != "0" {
        let name = byte_name(names.sanitize(key, Some(subtype)));
        if hooks.has_filter(&name, None, None)? != HasFilterResult::No {
            let out = hooks.apply_filters(
                &name,
                hv(&value),
                vec![
                    key_value(key),
                    HookValue::Str(object_type.to_owned()),
                    HookValue::Str(subtype.to_owned()),
                ],
            )?;
            return pv(&out);
        }
    }
    let name = byte_name(names.sanitize(key, None));
    let passed = hv(&value);
    let out = filter(hooks, name, passed, || {
        vec![key_value(key), HookValue::Str(object_type.to_owned())]
    })?;
    pv(&out)
}

/// `is_protected_meta()`: `inner`'s rule (usually [`UnfilteredProtection`](crate::UnfilteredProtection)), then the
/// `is_protected_meta` filter with `( $protected, $meta_key, $meta_type )`, read for truth (Decorator).
///
/// [`ProtectedMeta`] answers a bool and cannot fail, so a failing callback leaves the rule's answer and keeps its
/// error for [`take_hook_error`](Self::take_hook_error), as `Wpdb::take_hook_error` does. Callers that can carry
/// the error use [`try_is_protected`](Self::try_is_protected).
pub struct FilteredProtection {
    hooks: Rc<dyn HookApi>,
    inner: Rc<dyn ProtectedMeta>,
    error: RefCell<Option<HookError>>,
}

impl FilteredProtection {
    pub fn new(hooks: Rc<dyn HookApi>, inner: Rc<dyn ProtectedMeta>) -> FilteredProtection {
        FilteredProtection {
            hooks,
            inner,
            error: RefCell::new(None),
        }
    }

    /// `is_protected_meta()`, with a callback's failure as an error.
    pub fn try_is_protected(&self, key: &MetaKey, meta_type: &str) -> Result<bool, MetadataError> {
        let protected = self.inner.is_protected(key, meta_type);
        let out = filter(
            &*self.hooks,
            HookName::from("is_protected_meta"),
            HookValue::Bool(protected),
            || vec![key_value(key), HookValue::Str(meta_type.to_owned())],
        )?;
        Ok(crate::api::truthy(&pv(&out)?))
    }

    /// The failure of the last callback [`ProtectedMeta::is_protected`] met, once.
    pub fn take_hook_error(&self) -> Option<HookError> {
        self.error.borrow_mut().take()
    }
}

impl ProtectedMeta for FilteredProtection {
    fn is_protected(&self, key: &MetaKey, meta_type: &str) -> bool {
        match self.try_is_protected(key, meta_type) {
            Ok(protected) => protected,
            Err(e) => {
                let hook = match e {
                    MetadataError::Hook(h) => h,
                    other => HookError::message(other.to_string()),
                };
                *self.error.borrow_mut() = Some(hook);
                self.inner.is_protected(key, meta_type)
            }
        }
    }
}
