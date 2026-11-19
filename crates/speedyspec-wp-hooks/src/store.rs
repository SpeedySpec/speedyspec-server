//! The outbound port: where hooks, the run counts and the current-hook stack live.

use crate::{ArrayKey, HookValue, Partition, PrioritySlice, WpHook};

/// Which run count: `$wp_actions` or `$wp_filters`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Count {
    Actions,
    Filters,
}

/// `$wp_filter`, `$wp_actions`, `$wp_filters` and `$wp_current_filter`.
pub trait HookStore {
    // ----- read side

    /// `$wp_filter[$hook]`.
    fn hook(&self, name: &ArrayKey) -> Option<WpHook>;
    /// `isset($wp_filter[$hook])`.
    fn has_hook(&self, name: &ArrayKey) -> bool {
        self.hook(name).is_some()
    }
    /// The callbacks of `$wp_filter[$hook]`, by priority.
    fn callbacks(&self, name: &ArrayKey) -> Vec<(ArrayKey, PrioritySlice)> {
        self.hook(name)
            .map(|hook| {
                hook.state()
                    .callbacks
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect()
            })
            .unwrap_or_default()
    }
    /// Every hook in `$wp_filter`, in order.
    fn hooks(&self) -> Vec<(ArrayKey, WpHook)>;
    /// `$wp_actions` or `$wp_filters`, in order.
    fn counts(&self, kind: Count) -> Vec<(ArrayKey, i64)>;
    fn count(&self, kind: Count, name: &ArrayKey) -> Option<i64>;
    /// `$wp_current_filter`.
    fn current_stack(&self) -> Vec<HookValue>;
    /// The partition registrations go to now.
    fn partition(&self) -> Partition;

    // ----- write side

    /// `$wp_filter[$hook] = $hook_object`.
    fn insert(&self, name: ArrayKey, hook: WpHook);
    /// `unset($wp_filter[$hook])`.
    fn remove(&self, name: &ArrayKey);
    /// `$wp_filter = []`.
    fn clear(&self);
    fn push_current(&self, name: HookValue);
    fn pop_current(&self);
    /// `$wp_current_filter = $stack`.
    fn set_current_stack(&self, stack: Vec<HookValue>);
    /// `++$wp_actions[$hook]` (or 1).
    fn increment_did(&self, kind: Count, name: &ArrayKey);
    /// `$wp_actions[$hook] = $n`, or `unset(..)` for `None`.
    fn set_count(&self, kind: Count, name: &ArrayKey, value: Option<i64>);
}
