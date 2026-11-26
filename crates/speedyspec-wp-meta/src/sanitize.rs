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

/// `is_protected_meta( $meta_key, $meta_type )` as WordPress returns it: `rule`'s answer (the unfiltered rule,
/// usually [`UnfilteredProtection`](crate::UnfilteredProtection)), then the `is_protected_meta` filter, run once, with
/// `( $protected, $meta_key, $meta_type )`. The filter's answer comes back as it is (a callback may return a string).
pub fn protected_meta_answer(
    hooks: &dyn HookApi,
    rule: &dyn ProtectedMeta,
    key: &MetaKey,
    meta_type: &str,
) -> Result<PhpValue, MetadataError> {
    let protected = rule.is_protected(key, meta_type);
    let out = filter(
        hooks,
        HookName::from("is_protected_meta"),
        HookValue::Bool(protected),
        || vec![key_value(key), HookValue::Str(meta_type.to_owned())],
    )?;
    pv(&out)
}

/// [`protected_meta_answer`] read for truth, as `register_meta()` and the other callers read it.
pub fn is_protected_meta(
    hooks: &dyn HookApi,
    rule: &dyn ProtectedMeta,
    key: &MetaKey,
    meta_type: &str,
) -> Result<bool, MetadataError> {
    Ok(crate::api::truthy(&protected_meta_answer(
        hooks, rule, key, meta_type,
    )?))
}

/// [`is_protected_meta`] as a [`ProtectedMeta`] port, for code that asks the port (`register_meta()`'s
/// [`RegisterPorts`](crate::RegisterPorts)) (Decorator over the unfiltered rule).
///
/// [`ProtectedMeta`] answers a bool and cannot fail, so a failing callback leaves the rule's answer and keeps its
/// error. Each call clears what the previous one kept. Run the port's caller through [`checked`](Self::checked),
/// which turns a kept error into [`MetadataError::Hook`], payload intact.
pub struct FilteredProtection {
    hooks: Rc<dyn HookApi>,
    rule: Rc<dyn ProtectedMeta>,
    error: RefCell<Option<HookError>>,
}

impl FilteredProtection {
    /// `rule` is the unfiltered rule; the filter runs here, so `rule` must not apply it too.
    pub fn new(hooks: Rc<dyn HookApi>, rule: Rc<dyn ProtectedMeta>) -> FilteredProtection {
        FilteredProtection {
            hooks,
            rule,
            error: RefCell::new(None),
        }
    }

    /// `is_protected_meta()`, with a callback's failure as an error.
    pub fn try_is_protected(&self, key: &MetaKey, meta_type: &str) -> Result<bool, MetadataError> {
        is_protected_meta(&*self.hooks, &*self.rule, key, meta_type)
    }

    /// Runs `f` with this port, then reports the error a callback raised during it, if any.
    pub fn checked<T>(&self, f: impl FnOnce(&dyn ProtectedMeta) -> T) -> Result<T, MetadataError> {
        self.error.borrow_mut().take();
        let out = f(self);
        match self.error.borrow_mut().take() {
            Some(e) => Err(MetadataError::Hook(e)),
            None => Ok(out),
        }
    }

    /// The failure the last [`ProtectedMeta::is_protected`] call met, once.
    pub fn take_hook_error(&self) -> Option<HookError> {
        self.error.borrow_mut().take()
    }
}

impl ProtectedMeta for FilteredProtection {
    fn is_protected(&self, key: &MetaKey, meta_type: &str) -> bool {
        self.error.borrow_mut().take();
        match self.try_is_protected(key, meta_type) {
            Ok(protected) => protected,
            Err(e) => {
                let hook = match e {
                    MetadataError::Hook(h) => h,
                    other => HookError::message(other.to_string()),
                };
                *self.error.borrow_mut() = Some(hook);
                self.rule.is_protected(key, meta_type)
            }
        }
    }
}
