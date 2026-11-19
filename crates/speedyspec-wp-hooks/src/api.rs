//! The Hook API's functions (`plugin.php`), over a [`HookStore`].

use std::rc::Rc;

use crate::{
    ArrayKey, Callback, CallbackId, Count, HasFilterResult, HookError, HookStore, HookValue, WpHook,
};

/// WordPress's `_deprecated_hook()` (it stays outside the Hook API).
pub trait DeprecatedHooks {
    fn deprecated_hook(
        &self,
        hook: &HookValue,
        version: &HookValue,
        replacement: &HookValue,
        message: &HookValue,
    ) -> Result<(), HookError>;
}

/// Ignores deprecation notices.
pub struct IgnoreDeprecations;

impl DeprecatedHooks for IgnoreDeprecations {
    fn deprecated_hook(
        &self,
        _: &HookValue,
        _: &HookValue,
        _: &HookValue,
        _: &HookValue,
    ) -> Result<(), HookError> {
        Ok(())
    }
}

const ALL: &str = "all";

/// The functions of `plugin.php`. Every use-case handler runs through one of these.
pub struct HookApi {
    store: Rc<dyn HookStore>,
    deprecated: Rc<dyn DeprecatedHooks>,
}

impl HookApi {
    pub fn new(store: Rc<dyn HookStore>, deprecated: Rc<dyn DeprecatedHooks>) -> HookApi {
        HookApi { store, deprecated }
    }

    pub fn store(&self) -> &Rc<dyn HookStore> {
        &self.store
    }

    fn all(&self) -> Option<WpHook> {
        self.store.hook(&ArrayKey::Str(ALL.into()))
    }

    pub fn add_filter(
        &self,
        hook: &HookValue,
        callback: Callback,
        priority: &HookValue,
        accepted_args: i64,
    ) -> Result<bool, HookError> {
        let key = ArrayKey::from_value(hook)?;
        let existing = self.store.hook(&key);
        let target = match existing {
            Some(target) => target,
            None => {
                let target = WpHook::new();
                self.store.insert(key, target.clone());
                target
            }
        };
        target.add_filter(callback, priority, accepted_args, self.store.partition())?;
        Ok(true)
    }

    fn run_all(&self, args: Vec<HookValue>) -> Result<(), HookError> {
        match self.all() {
            Some(all) => all.do_all_hook(&args),
            None => Ok(()),
        }
    }

    /// The shared body of `apply_filters()` and `apply_filters_ref_array()`.
    fn filter(
        &self,
        hook: &HookValue,
        count: Count,
        all_args: impl FnOnce() -> Vec<HookValue>,
        value: HookValue,
        args: Vec<HookValue>,
    ) -> Result<HookValue, HookError> {
        let key = ArrayKey::from_value(hook)?;
        self.store.increment_did(count, &key);
        if self.all().is_some() {
            self.store.push_current(hook.clone());
            self.run_all(all_args())?;
        }
        let Some(target) = self.store.hook(&key) else {
            if self.all().is_some() {
                self.store.pop_current();
            }
            return Ok(value);
        };
        if self.all().is_none() {
            self.store.push_current(hook.clone());
        }
        let filtered = target.apply_filters(value, args)?;
        self.store.pop_current();
        Ok(filtered)
    }

    pub fn apply_filters(
        &self,
        hook: &HookValue,
        value: HookValue,
        args: Vec<HookValue>,
    ) -> Result<HookValue, HookError> {
        let mut all = vec![hook.clone(), value.clone()];
        all.extend(args.iter().cloned());
        let mut with_value = vec![value.clone()];
        with_value.extend(args);
        self.filter(hook, Count::Filters, || all, value, with_value)
    }

    pub fn apply_filters_ref_array(
        &self,
        hook: &HookValue,
        args: Vec<HookValue>,
    ) -> Result<HookValue, HookError> {
        let all = vec![hook.clone(), list(&args)];
        let value = args.first().cloned().unwrap_or(HookValue::Null);
        self.filter(hook, Count::Filters, || all, value, args)
    }

    pub fn has_filter(
        &self,
        hook: &HookValue,
        callback: Option<&CallbackId>,
        priority: Option<i64>,
    ) -> Result<HasFilterResult, HookError> {
        let key = ArrayKey::from_value(hook)?;
        Ok(match self.store.hook(&key) {
            None => HasFilterResult::No,
            Some(target) => target.has_filter(callback.map(|id| &id.0), priority),
        })
    }

    pub fn remove_filter(
        &self,
        hook: &HookValue,
        callback: CallbackId,
        priority: &HookValue,
    ) -> Result<bool, HookError> {
        let key = ArrayKey::from_value(hook)?;
        let Some(target) = self.store.hook(&key) else {
            return Ok(false);
        };
        let removed = target.remove_filter(callback.0.as_deref(), priority)?;
        if target.state().callbacks.is_empty() {
            self.store.remove(&key);
        }
        Ok(removed)
    }

    pub fn remove_all_filters(
        &self,
        hook: &HookValue,
        priority: Option<&HookValue>,
    ) -> Result<bool, HookError> {
        let key = ArrayKey::from_value(hook)?;
        if let Some(target) = self.store.hook(&key) {
            target.remove_all_filters(priority)?;
            if !target.has_filters() {
                self.store.remove(&key);
            }
        }
        Ok(true)
    }

    /// `current_filter()`: `None` is PHP's `false`.
    pub fn current_filter(&self) -> Option<HookValue> {
        self.store.current_stack().last().cloned()
    }

    pub fn doing_filter(&self, hook: Option<&HookValue>) -> bool {
        let stack = self.store.current_stack();
        match hook {
            None => !stack.is_empty(),
            Some(hook) => stack.iter().any(|name| name == hook),
        }
    }

    pub fn did_filter(&self, hook: &HookValue) -> Result<i64, HookError> {
        Ok(self
            .store
            .count(Count::Filters, &ArrayKey::from_value(hook)?)
            .unwrap_or(0))
    }

    pub fn did_action(&self, hook: &HookValue) -> Result<i64, HookError> {
        Ok(self
            .store
            .count(Count::Actions, &ArrayKey::from_value(hook)?)
            .unwrap_or(0))
    }

    /// The shared body of `do_action()` and `do_action_ref_array()`.
    fn action(
        &self,
        hook: &HookValue,
        all_args: impl FnOnce() -> Vec<HookValue>,
        args: impl FnOnce() -> Vec<HookValue>,
    ) -> Result<(), HookError> {
        let key = ArrayKey::from_value(hook)?;
        self.store.increment_did(Count::Actions, &key);
        if self.all().is_some() {
            self.store.push_current(hook.clone());
            self.run_all(all_args())?;
        }
        let Some(target) = self.store.hook(&key) else {
            if self.all().is_some() {
                self.store.pop_current();
            }
            return Ok(());
        };
        if self.all().is_none() {
            self.store.push_current(hook.clone());
        }
        target.do_action(args())?;
        self.store.pop_current();
        Ok(())
    }

    pub fn do_action(&self, hook: &HookValue, args: Vec<HookValue>) -> Result<(), HookError> {
        let mut all = vec![hook.clone()];
        all.extend(args.iter().cloned());
        self.action(
            hook,
            || all,
            || {
                let mut args = args;
                if args.is_empty() {
                    args.push(HookValue::Str(String::new()));
                } else if let Some(object) = single_object(&args[0]) {
                    // Backward compatibility for PHP4-style passing of `array( &$this )`.
                    args[0] = object;
                }
                args
            },
        )
    }

    pub fn do_action_ref_array(
        &self,
        hook: &HookValue,
        args: Vec<HookValue>,
    ) -> Result<(), HookError> {
        let all = vec![hook.clone(), list(&args)];
        self.action(hook, || all, || args)
    }

    pub fn apply_filters_deprecated(
        &self,
        hook: &HookValue,
        args: Vec<HookValue>,
        version: &HookValue,
        replacement: &HookValue,
        message: &HookValue,
    ) -> Result<HookValue, HookError> {
        if self.has_filter(hook, None, None)? == HasFilterResult::No {
            return Ok(args.first().cloned().unwrap_or(HookValue::Null));
        }
        self.deprecated
            .deprecated_hook(hook, version, replacement, message)?;
        self.apply_filters_ref_array(hook, args)
    }

    pub fn do_action_deprecated(
        &self,
        hook: &HookValue,
        args: Vec<HookValue>,
        version: &HookValue,
        replacement: &HookValue,
        message: &HookValue,
    ) -> Result<(), HookError> {
        if self.has_filter(hook, None, None)? == HasFilterResult::No {
            return Ok(());
        }
        self.deprecated
            .deprecated_hook(hook, version, replacement, message)?;
        self.do_action_ref_array(hook, args)
    }

    /// `$wp_filter[$hook]->current_priority()`; `None` when the hook is missing or not running.
    pub fn current_priority(&self, hook: &HookValue) -> Result<Option<ArrayKey>, HookError> {
        Ok(self
            .store
            .hook(&ArrayKey::from_value(hook)?)
            .and_then(|target| target.current_priority()))
    }
}

/// A PHP list of `args`.
fn list(args: &[HookValue]) -> HookValue {
    HookValue::Array(
        args.iter()
            .enumerate()
            .map(|(i, v)| (speedyspec_hooks::Key::Int(i as i64), v.clone()))
            .collect(),
    )
}

/// The object of `[$object]` (an array of one object at key 0).
fn single_object(arg: &HookValue) -> Option<HookValue> {
    match arg {
        HookValue::Array(entries) if entries.len() == 1 => match &entries[0] {
            (speedyspec_hooks::Key::Int(0), object @ HookValue::Opaque(_)) => Some(object.clone()),
            _ => None,
        },
        _ => None,
    }
}
