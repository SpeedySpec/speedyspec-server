//! The Hook API's functions (`plugin.php`), over a [`HookStore`].

use std::rc::Rc;

use std::sync::LazyLock;

use crate::{
    ArrayKey, Callback, CallbackId, Count, HasFilterResult, HookError, HookKey, HookName,
    HookStore, HookValue, WpHook,
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

/// The `all` hook's key, hashed once.
static ALL: LazyLock<HookKey> = LazyLock::new(|| HookKey::new(ArrayKey::Str("all".into())));

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
        self.store.hook(&ALL)
    }

    /// The fast path of `apply_filters()` (`count` [`Count::Filters`]) and `do_action()` ([`Count::Actions`]):
    /// when nothing would run for `hook` (it has no callbacks and there is no `all` hook), the run is counted as
    /// WordPress counts it and this answers true; the caller hands the value straight back, without building the
    /// arguments. False means nothing was done: run the use case.
    pub fn skip_if_empty(&self, hook: &HookName, count: Count) -> Result<bool, HookError> {
        let key = hook.key()?;
        if self.store.has_hook(key) || self.store.has_hook(&ALL) {
            return Ok(false);
        }
        self.store.increment_did(count, key);
        Ok(true)
    }

    pub fn add_filter(
        &self,
        hook: &HookName,
        callback: Callback,
        priority: &HookValue,
        accepted_args: i64,
    ) -> Result<bool, HookError> {
        let key = hook.key()?;
        let existing = self.store.hook(key);
        let target = match existing {
            Some(target) => target,
            None => {
                let target = WpHook::new();
                self.store.insert(key.clone(), target.clone());
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
    /// `args` makes every argument, the value first; `all_args` the `all` hook's arguments from them. Neither is
    /// called when nothing runs.
    fn filter(
        &self,
        hook: &HookName,
        count: Count,
        value: HookValue,
        args: impl FnOnce() -> Vec<HookValue>,
        all_args: impl FnOnce(&[HookValue]) -> Vec<HookValue>,
    ) -> Result<HookValue, HookError> {
        let key = hook.key()?;
        self.store.increment_did(count, key);
        let mut args = Some(args);
        let mut built = None;
        let all = self.all().is_some();
        if all {
            self.store.push_current(hook.value().clone());
            let made = args.take().map(|make| make()).unwrap_or_default();
            let made = built.insert(made);
            self.run_all(all_args(made))?;
        }
        let Some(target) = self.store.hook(key) else {
            if all {
                self.store.pop_current();
            }
            return Ok(value);
        };
        if !all {
            self.store.push_current(hook.value().clone());
        }
        let args = match (built, args) {
            (Some(built), _) => built,
            (None, Some(make)) => make(),
            (None, None) => Vec::new(),
        };
        let filtered = target.apply_filters(value, args)?;
        self.store.pop_current();
        Ok(filtered)
    }

    /// `apply_filters()`. The argument lists are built only for what runs.
    ///
    /// The result is what the last callback returned, as it returned it: a value a PHP callback returns passes
    /// through unconverted (a `HookValue::Opaque`), so a Rust caller that looks at it reads it with
    /// [`HookValue::read`]. Rust callbacks always get read values.
    pub fn apply_filters(
        &self,
        hook: &HookName,
        value: HookValue,
        args: Vec<HookValue>,
    ) -> Result<HookValue, HookError> {
        let first = value.clone();
        let with_value = move || {
            let mut with_value = Vec::with_capacity(args.len() + 1);
            with_value.push(first);
            with_value.extend(args);
            with_value
        };
        let all_args = |with_value: &[HookValue]| {
            let mut all = Vec::with_capacity(with_value.len() + 1);
            all.push(hook.value().clone());
            all.extend(with_value.iter().cloned());
            all
        };
        self.filter(hook, Count::Filters, value, with_value, all_args)
    }

    pub fn apply_filters_ref_array(
        &self,
        hook: &HookName,
        args: Vec<HookValue>,
    ) -> Result<HookValue, HookError> {
        let value = args.first().cloned().unwrap_or(HookValue::Null);
        let all_args = |args: &[HookValue]| vec![hook.value().clone(), list(args)];
        self.filter(hook, Count::Filters, value, move || args, all_args)
    }

    pub fn has_filter(
        &self,
        hook: &HookName,
        callback: Option<&CallbackId>,
        priority: Option<i64>,
    ) -> Result<HasFilterResult, HookError> {
        let key = hook.key()?;
        Ok(match self.store.hook(key) {
            None => HasFilterResult::No,
            Some(target) => target.has_filter(callback.map(|id| &id.0), priority),
        })
    }

    pub fn remove_filter(
        &self,
        hook: &HookName,
        callback: CallbackId,
        priority: &HookValue,
    ) -> Result<bool, HookError> {
        let key = hook.key()?;
        let Some(target) = self.store.hook(key) else {
            return Ok(false);
        };
        let removed = target.remove_filter(callback.0.as_deref(), priority)?;
        if target.state().callbacks.is_empty() {
            self.store.remove(key);
        }
        Ok(removed)
    }

    pub fn remove_all_filters(
        &self,
        hook: &HookName,
        priority: Option<&HookValue>,
    ) -> Result<bool, HookError> {
        let key = hook.key()?;
        if let Some(target) = self.store.hook(key) {
            target.remove_all_filters(priority)?;
            if !target.has_filters() {
                self.store.remove(key);
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

    pub fn did_filter(&self, hook: &HookName) -> Result<i64, HookError> {
        Ok(self.store.count(Count::Filters, hook.key()?).unwrap_or(0))
    }

    pub fn did_action(&self, hook: &HookName) -> Result<i64, HookError> {
        Ok(self.store.count(Count::Actions, hook.key()?).unwrap_or(0))
    }

    /// The shared body of `do_action()` and `do_action_ref_array()`: `all_args` makes the `all` hook's arguments
    /// from `args`, `prepare` the callbacks' arguments. Neither is called when nothing runs.
    fn action(
        &self,
        hook: &HookName,
        args: Vec<HookValue>,
        all_args: impl FnOnce(&[HookValue]) -> Vec<HookValue>,
        prepare: impl FnOnce(Vec<HookValue>) -> Vec<HookValue>,
    ) -> Result<(), HookError> {
        let key = hook.key()?;
        self.store.increment_did(Count::Actions, key);
        let all = self.all().is_some();
        if all {
            self.store.push_current(hook.value().clone());
            self.run_all(all_args(&args))?;
        }
        let Some(target) = self.store.hook(key) else {
            if all {
                self.store.pop_current();
            }
            return Ok(());
        };
        if !all {
            self.store.push_current(hook.value().clone());
        }
        target.do_action(prepare(args))?;
        self.store.pop_current();
        Ok(())
    }

    pub fn do_action(&self, hook: &HookName, args: Vec<HookValue>) -> Result<(), HookError> {
        let all = |args: &[HookValue]| {
            let mut all = Vec::with_capacity(args.len() + 1);
            all.push(hook.value().clone());
            all.extend(args.iter().cloned());
            all
        };
        let prepare = |mut args: Vec<HookValue>| {
            if args.is_empty() {
                args.push(HookValue::Str(String::new()));
            } else if let Some(object) = single_object(&args[0]) {
                // Backward compatibility for PHP4-style passing of `array( &$this )`.
                args[0] = object;
            }
            args
        };
        self.action(hook, args, all, prepare)
    }

    pub fn do_action_ref_array(
        &self,
        hook: &HookName,
        args: Vec<HookValue>,
    ) -> Result<(), HookError> {
        let all = |args: &[HookValue]| vec![hook.value().clone(), list(args)];
        self.action(hook, args, all, |args| args)
    }

    pub fn apply_filters_deprecated(
        &self,
        hook: &HookName,
        args: Vec<HookValue>,
        version: &HookValue,
        replacement: &HookValue,
        message: &HookValue,
    ) -> Result<HookValue, HookError> {
        if self.has_filter(hook, None, None)? == HasFilterResult::No {
            return Ok(args.first().cloned().unwrap_or(HookValue::Null));
        }
        self.deprecated
            .deprecated_hook(hook.value(), version, replacement, message)?;
        self.apply_filters_ref_array(hook, args)
    }

    pub fn do_action_deprecated(
        &self,
        hook: &HookName,
        args: Vec<HookValue>,
        version: &HookValue,
        replacement: &HookValue,
        message: &HookValue,
    ) -> Result<(), HookError> {
        if self.has_filter(hook, None, None)? == HasFilterResult::No {
            return Ok(());
        }
        self.deprecated
            .deprecated_hook(hook.value(), version, replacement, message)?;
        self.do_action_ref_array(hook, args)
    }

    /// `$wp_filter[$hook]->current_priority()`; `None` when the hook is missing or not running.
    pub fn current_priority(&self, hook: &HookName) -> Result<Option<ArrayKey>, HookError> {
        Ok(self
            .store
            .hook(hook.key()?)
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
