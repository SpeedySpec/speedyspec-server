//! `PhpValueHooks`: the Hook API as the post read use cases see it (Decorator).
//!
//! The use cases carry objects (a `WP_Post`, a counts `stdClass`) as `PhpValue`s inside the hook values
//! (`OpaquePhpValue`), which PHP cannot read, and they read a callback's answer back as a `PhpValue`, which a live PHP
//! object is not. This decorator makes each carried value a PHP value on the way into a hook, and carries each PHP
//! object in an answer on the way out: a `stdClass` copied with its properties, any other object kept live, so the
//! answer handed back to PHP is the very object the callback returned. A value without objects passes through.
//!
//! It also refuses every hook while a port has parked a failure: the use case stops at its next hook, before
//! anything else is filtered, fired or written, as WordPress stops where the exception is thrown.
//!
//! [`PhpValueOptions`] does the same for the options API: a stored option may be any PHP value, an object included
//! (`sticky_posts` holding a `stdClass`, say), which the use cases read as a carried `PhpValue`.

use std::rc::Rc;

use speedyspec_wp_hooks_contracts::{
    Callback, CallbackId, Count, HasFilterResult, HookApi, HookError, HookName, HookValue,
};
use speedyspec_wp_options_contracts::{Options, OptionsError};

use crate::adapters::is_parked;
use crate::codec::{Thrown, hook_in, hook_out};

/// A conversion PHP failed: its exception is the payload the native function rethrows.
fn failed(thrown: Thrown) -> HookError {
    HookError {
        message: "a post value could not cross into PHP".into(),
        payload: thrown
            .0
            .map(|exception| Rc::new(exception) as Rc<dyn std::any::Any>),
    }
}

/// Fails while a port's failure is parked; the native function rethrows the parked failure itself.
fn not_parked() -> Result<(), HookError> {
    if is_parked() {
        return Err(HookError::message(
            "a posts port failed: the work stops before the next hook",
        ));
    }
    Ok(())
}

fn into_php(args: Vec<HookValue>) -> Result<Vec<HookValue>, HookError> {
    args.into_iter()
        .map(|v| hook_in(v).map_err(failed))
        .collect()
}

/// The Hook API with the post read use cases' values converted at the edge.
pub struct PhpValueHooks {
    inner: Rc<dyn HookApi>,
}

impl PhpValueHooks {
    pub fn new(inner: Rc<dyn HookApi>) -> PhpValueHooks {
        PhpValueHooks { inner }
    }
}

impl HookApi for PhpValueHooks {
    fn skip_if_empty(&self, hook: &HookName, count: Count) -> Result<bool, HookError> {
        not_parked()?;
        self.inner.skip_if_empty(hook, count)
    }

    fn add_filter(
        &self,
        hook: &HookName,
        callback: Callback,
        priority: &HookValue,
        accepted_args: i64,
    ) -> Result<bool, HookError> {
        not_parked()?;
        self.inner
            .add_filter(hook, callback, priority, accepted_args)
    }

    fn has_filter(
        &self,
        hook: &HookName,
        callback: Option<&CallbackId>,
        priority: Option<i64>,
    ) -> Result<HasFilterResult, HookError> {
        not_parked()?;
        self.inner.has_filter(hook, callback, priority)
    }

    fn remove_filter(
        &self,
        hook: &HookName,
        callback: CallbackId,
        priority: &HookValue,
    ) -> Result<bool, HookError> {
        not_parked()?;
        self.inner.remove_filter(hook, callback, priority)
    }

    fn apply_filters(
        &self,
        hook: &HookName,
        value: HookValue,
        args: Vec<HookValue>,
    ) -> Result<HookValue, HookError> {
        not_parked()?;
        let value = hook_in(value).map_err(failed)?;
        let out = self.inner.apply_filters(hook, value, into_php(args)?)?;
        hook_out(out).map_err(failed)
    }

    fn do_action(&self, hook: &HookName, args: Vec<HookValue>) -> Result<(), HookError> {
        not_parked()?;
        self.inner.do_action(hook, into_php(args)?)
    }

    fn do_action_ref_array(&self, hook: &HookName, args: Vec<HookValue>) -> Result<(), HookError> {
        not_parked()?;
        self.inner.do_action_ref_array(hook, into_php(args)?)
    }

    fn do_action_deprecated(
        &self,
        hook: &HookName,
        args: Vec<HookValue>,
        version: &HookValue,
        replacement: &HookValue,
        message: &HookValue,
    ) -> Result<(), HookError> {
        not_parked()?;
        self.inner
            .do_action_deprecated(hook, into_php(args)?, version, replacement, message)
    }
}

/// The options API with the post use cases' values converted at the edge (Decorator): each carried value a PHP value
/// on the way in, each PHP object in an answer carried on the way out.
pub struct PhpValueOptions {
    inner: Rc<dyn Options>,
}

impl PhpValueOptions {
    pub fn new(inner: Rc<dyn Options>) -> PhpValueOptions {
        PhpValueOptions { inner }
    }
}

fn option_in(value: HookValue) -> Result<HookValue, OptionsError> {
    hook_in(value).map_err(|t| OptionsError::Hook(failed(t)))
}

impl Options for PhpValueOptions {
    fn get_option(
        &self,
        name: &str,
        default: Option<HookValue>,
    ) -> Result<HookValue, OptionsError> {
        let default = default.map(option_in).transpose()?;
        let value = self.inner.get_option(name, default)?;
        hook_out(value).map_err(|t| OptionsError::Hook(failed(t)))
    }

    fn add_option(
        &self,
        name: &str,
        value: HookValue,
        deprecated: HookValue,
        autoload: HookValue,
    ) -> Result<bool, OptionsError> {
        self.inner
            .add_option(name, option_in(value)?, deprecated, autoload)
    }

    fn update_option(
        &self,
        name: &str,
        value: HookValue,
        autoload: HookValue,
    ) -> Result<bool, OptionsError> {
        self.inner.update_option(name, option_in(value)?, autoload)
    }

    fn delete_option(&self, name: &str) -> Result<bool, OptionsError> {
        self.inner.delete_option(name)
    }
}
