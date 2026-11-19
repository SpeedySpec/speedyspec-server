//! The outbound port: where hooks, the run counts and the current-hook stack live.

use crate::{ArrayKey, HookKey, HookValue, Partition, PrioritySlice, WpHook};

/// Which run count: `$wp_actions` or `$wp_filters`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Count {
    Actions,
    Filters,
}

/// `$wp_filter`, `$wp_actions`, `$wp_filters` and `$wp_current_filter`. Hooks and counts are looked up by
/// [`HookKey`], whose hash is PHP's own: a store keeps it rather than hashing the name again.
pub trait HookStore {
    // ----- read side

    /// `$wp_filter[$hook]`.
    fn hook(&self, name: &HookKey) -> Option<WpHook>;
    /// `isset($wp_filter[$hook])`.
    fn has_hook(&self, name: &HookKey) -> bool {
        self.hook(name).is_some()
    }
    /// The callbacks of `$wp_filter[$hook]`, by priority.
    fn callbacks(&self, name: &HookKey) -> Vec<(ArrayKey, PrioritySlice)> {
        self.hook(name)
            .map(|hook| {
                hook.state()
                    .callbacks
                    .iter()
                    .map(|(k, v)| (k.clone(), PrioritySlice::clone(v)))
                    .collect()
            })
            .unwrap_or_default()
    }
    /// Every hook in `$wp_filter`, in order.
    fn hooks(&self) -> Vec<(ArrayKey, WpHook)>;
    /// `$wp_actions` or `$wp_filters`, in order.
    fn counts(&self, kind: Count) -> Vec<(ArrayKey, i64)>;
    fn count(&self, kind: Count, name: &HookKey) -> Option<i64>;
    /// `$wp_current_filter`.
    fn current_stack(&self) -> Vec<HookValue>;
    /// The partition registrations go to now.
    fn partition(&self) -> Partition;

    // ----- write side

    /// `$wp_filter[$hook] = $hook_object`.
    fn insert(&self, name: HookKey, hook: WpHook);
    /// `unset($wp_filter[$hook])`.
    fn remove(&self, name: &HookKey);
    /// `$wp_filter = []`.
    fn clear(&self);
    fn push_current(&self, name: HookValue);
    fn pop_current(&self);
    /// `$wp_current_filter = $stack`.
    fn set_current_stack(&self, stack: Vec<HookValue>);
    /// `++$wp_actions[$hook]` (or 1).
    fn increment_did(&self, kind: Count, name: &HookKey);
    /// `$wp_actions[$hook] = $n`, or `unset(..)` for `None`.
    fn set_count(&self, kind: Count, name: &HookKey, value: Option<i64>);
}
