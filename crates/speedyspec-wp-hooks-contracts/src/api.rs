//! The Hook API port: `plugin.php`, as other seams call it.

use crate::{ArrayKey, Callback, CallbackId, HookError, HookName, HookValue};

/// Which run count: `$wp_actions` or `$wp_filters`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Count {
    Actions,
    Filters,
}

/// What `has_filter()` answers: `false`, `true`, or the priority a callback is registered at.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HasFilterResult {
    No,
    Any,
    At(ArrayKey),
}

/// WordPress's plugin.php, as other seams call it. An API port: `Callback` holds Rust closures, so Rust replaces it;
/// PHP replaces the `HookStore` behind it.
pub trait HookApi {
    /// options, wpdb, phase 3: count the run and answer true when nothing would run.
    fn skip_if_empty(&self, hook: &HookName, count: Count) -> Result<bool, HookError>;
    /// options (settings, autoload), phase 3 (register_meta).
    fn add_filter(
        &self,
        hook: &HookName,
        callback: Callback,
        priority: &HookValue,
        accepted_args: i64,
    ) -> Result<bool, HookError>;
    /// phase 3 (sanitize_meta, register_meta).
    fn has_filter(
        &self,
        hook: &HookName,
        callback: Option<&CallbackId>,
        priority: Option<i64>,
    ) -> Result<HasFilterResult, HookError>;
    /// options (settings), phase 3 (unregister_meta_key).
    fn remove_filter(
        &self,
        hook: &HookName,
        callback: CallbackId,
        priority: &HookValue,
    ) -> Result<bool, HookError>;
    /// options, cron, phase 3.
    fn apply_filters(
        &self,
        hook: &HookName,
        value: HookValue,
        args: Vec<HookValue>,
    ) -> Result<HookValue, HookError>;
    /// options, cron, phase 3.
    fn do_action(&self, hook: &HookName, args: Vec<HookValue>) -> Result<(), HookError>;
    /// cron.
    fn do_action_ref_array(&self, hook: &HookName, args: Vec<HookValue>) -> Result<(), HookError>;
    /// options (deprecated transient hooks).
    fn do_action_deprecated(
        &self,
        hook: &HookName,
        args: Vec<HookValue>,
        version: &HookValue,
        replacement: &HookValue,
        message: &HookValue,
    ) -> Result<(), HookError>;
}
