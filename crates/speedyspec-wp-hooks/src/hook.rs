//! `WP_Hook`: one hook's callbacks by priority, and how running iterations follow changes ([`HookIteration`]).
//!
//! This is a literal port of WordPress 7.1's `class-wp-hook.php`, PHP's array semantics included:
//!
//! - priorities are array keys, kept sorted with `ksort(SORT_NUMERIC)` whenever a new one is added;
//! - each running `apply_filters()` keeps its own copy of the priority list with an internal pointer;
//!   `resort_active_iterations()` re-reads the list and re-positions every running pointer when priorities are
//!   added or removed;
//! - the callbacks of one priority are iterated over a snapshot (PHP's `foreach` by value), so a callback added or
//!   removed at the running priority takes effect on the next run;
//! - a callback that fails leaves the hook as PHP would after an exception: nesting level, iteration and
//!   `doing_action` are not unwound.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use indexmap::IndexMap;

use crate::{ArrayKey, Callback, HookError, HookValue, PriorityOrder};

/// Which partition of a store a callback belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Partition {
    /// Registered for the life of the worker.
    Worker,
    /// Registered during a request; dropped when it ends.
    Request,
}

/// One registered callback: `['function' => .., 'accepted_args' => ..]`.
#[derive(Clone, Debug)]
pub struct Entry {
    pub callback: Callback,
    pub accepted_args: i64,
    pub partition: Partition,
}

/// The callbacks of one priority, by id, in registration order.
pub type PrioritySlice = IndexMap<String, Entry>;

/// A running iteration's copy of the priority list and its internal pointer (`None`: past the end).
#[derive(Clone, Debug, Default)]
struct Iteration {
    list: Vec<ArrayKey>,
    pos: Option<usize>,
}

impl Iteration {
    fn new(list: Vec<ArrayKey>) -> Iteration {
        let pos = (!list.is_empty()).then_some(0);
        Iteration { list, pos }
    }

    fn current(&self) -> Option<ArrayKey> {
        self.pos.and_then(|p| self.list.get(p).cloned())
    }

    /// PHP's `next()`: false once past the end.
    fn next(&mut self) -> bool {
        match self.pos {
            Some(p) if p + 1 < self.list.len() => {
                self.pos = Some(p + 1);
                true
            }
            _ => {
                self.pos = None;
                false
            }
        }
    }

    fn prev(&mut self) -> Option<ArrayKey> {
        match self.pos {
            Some(p) if p > 0 => {
                self.pos = Some(p - 1);
                self.current()
            }
            _ => {
                self.pos = None;
                None
            }
        }
    }

    fn end(&mut self) -> Option<ArrayKey> {
        self.pos = self.list.len().checked_sub(1);
        self.current()
    }

    fn reset(&mut self) {
        self.pos = (!self.list.is_empty()).then_some(0);
    }
}

/// The state of one `WP_Hook`.
#[derive(Debug, Default)]
pub struct HookState {
    /// `$callbacks`, sorted by priority.
    pub callbacks: IndexMap<ArrayKey, PrioritySlice>,
    priorities: Vec<ArrayKey>,
    /// Running iterations by nesting level (a PHP array: insertion order is level order).
    iterations: BTreeMap<i64, Iteration>,
    current_priority: BTreeMap<i64, Option<ArrayKey>>,
    nesting_level: i64,
    doing_action: bool,
}

/// What `has_filter()` answers: `false`, `true`, or the priority a callback is registered at.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HasFilterResult {
    No,
    Any,
    At(ArrayKey),
}

/// WordPress's iterator rules when the running hook changes (`resort_active_iterations()`).
pub struct HookIteration;

impl HookIteration {
    fn resort(state: &mut HookState, new_priority: Option<&HookValue>, priority_existed: bool) {
        let new_priorities = state.priorities.clone();
        if new_priorities.is_empty() {
            for iteration in state.iterations.values_mut() {
                *iteration = Iteration::new(Vec::new());
            }
            return;
        }
        let min = PriorityOrder::min(&new_priorities)
            .cloned()
            .unwrap_or(ArrayKey::Int(0));
        let levels: Vec<i64> = state.iterations.keys().copied().collect();
        for index in levels {
            let current_priority = state.current_priority.get(&index).cloned().flatten();
            let iteration = state.iterations.get_mut(&index).expect("a running level");
            let Some(current) = iteration.current() else {
                continue;
            };
            *iteration = Iteration::new(new_priorities.clone());
            if PriorityOrder::php_less(&current, &min) {
                iteration.list.insert(0, current);
                iteration.reset();
                continue;
            }
            while iteration
                .current()
                .is_some_and(|p| PriorityOrder::php_less(&p, &current))
            {
                if !iteration.next() {
                    break;
                }
            }
            let is_current = match (new_priority, &current_priority) {
                (Some(new), Some(running)) => ArrayKey::identical(new, running),
                _ => false,
            };
            if is_current && !priority_existed {
                let prev = if iteration.current().is_none() {
                    iteration.end()
                } else {
                    iteration.prev()
                };
                match prev {
                    None => iteration.reset(),
                    Some(prev) => {
                        if !new_priority.is_some_and(|new| ArrayKey::identical(new, &prev)) {
                            iteration.next();
                        }
                    }
                }
            }
        }
    }
}

/// A `WP_Hook`, shared: `$wp_filter[$name]` and any PHP object for it are the same hook.
#[derive(Clone, Debug, Default)]
pub struct WpHook(Rc<RefCell<HookState>>);

impl PartialEq for WpHook {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

/// `null` priorities are 0, as `add_filter()` and `remove_filter()` treat them.
fn priority_or_zero(priority: &HookValue) -> HookValue {
    if matches!(priority, HookValue::Null) {
        HookValue::Int(0)
    } else {
        priority.clone()
    }
}

impl WpHook {
    pub fn new() -> WpHook {
        WpHook::default()
    }

    pub fn state(&self) -> std::cell::Ref<'_, HookState> {
        self.0.borrow()
    }

    pub fn add_filter(
        &self,
        callback: Callback,
        priority: &HookValue,
        accepted_args: i64,
        partition: Partition,
    ) -> Result<(), HookError> {
        let priority = priority_or_zero(priority);
        let Some(id) = callback.id().map(str::to_string) else {
            return Ok(());
        };
        let key = ArrayKey::from_value(&priority)?;
        let mut state = self.0.borrow_mut();
        let existed = state.callbacks.contains_key(&key);
        state.callbacks.entry(key).or_default().insert(
            id,
            Entry {
                callback,
                accepted_args,
                partition,
            },
        );
        if !existed && state.callbacks.len() > 1 {
            state
                .callbacks
                .sort_by(|a, _, b, _| PriorityOrder::compare(a, b));
        }
        state.priorities = state.callbacks.keys().cloned().collect();
        if state.nesting_level > 0 {
            HookIteration::resort(&mut state, Some(&priority), existed);
        }
        Ok(())
    }

    pub fn remove_filter(&self, id: Option<&str>, priority: &HookValue) -> Result<bool, HookError> {
        let key = ArrayKey::from_value(&priority_or_zero(priority))?;
        let Some(id) = id else {
            return Ok(false);
        };
        let mut state = self.0.borrow_mut();
        let Some(slice) = state.callbacks.get_mut(&key) else {
            return Ok(false);
        };
        if slice.shift_remove(id).is_none() {
            return Ok(false);
        }
        if slice.is_empty() {
            state.callbacks.shift_remove(&key);
            state.priorities = state.callbacks.keys().cloned().collect();
            if state.nesting_level > 0 {
                HookIteration::resort(&mut state, None, false);
            }
        }
        Ok(true)
    }

    /// `has_filter($hook, $callback, $priority)`: `callback` is `None` for `false` (any callback); `priority` is
    /// given only when it is an integer.
    pub fn has_filter(
        &self,
        callback: Option<&Option<String>>,
        priority: Option<i64>,
    ) -> HasFilterResult {
        let Some(id) = callback else {
            return if self.has_filters() {
                HasFilterResult::Any
            } else {
                HasFilterResult::No
            };
        };
        let Some(id) = id.as_deref().filter(|id| !id.is_empty() && *id != "0") else {
            return HasFilterResult::No;
        };
        let state = self.0.borrow();
        if let Some(priority) = priority {
            let found = state
                .callbacks
                .get(&ArrayKey::Int(priority))
                .is_some_and(|slice| slice.contains_key(id));
            return if found {
                HasFilterResult::Any
            } else {
                HasFilterResult::No
            };
        }
        for (priority, slice) in &state.callbacks {
            if slice.contains_key(id) {
                return HasFilterResult::At(priority.clone());
            }
        }
        HasFilterResult::No
    }

    pub fn has_filters(&self) -> bool {
        self.0
            .borrow()
            .callbacks
            .values()
            .any(|slice| !slice.is_empty())
    }

    /// `remove_all_filters($priority)`: `None` for `false` (every priority).
    pub fn remove_all_filters(&self, priority: Option<&HookValue>) -> Result<(), HookError> {
        let mut state = self.0.borrow_mut();
        if state.callbacks.is_empty() {
            return Ok(());
        }
        match priority {
            None => {
                state.callbacks.clear();
                state.priorities.clear();
            }
            Some(priority) => {
                let key = ArrayKey::from_value(priority)?;
                if state.callbacks.shift_remove(&key).is_some() {
                    state.priorities = state.callbacks.keys().cloned().collect();
                }
            }
        }
        if state.nesting_level > 0 {
            HookIteration::resort(&mut state, None, false);
        }
        Ok(())
    }

    /// Runs the callbacks over `value`; `args` is every argument, `value` first.
    pub fn apply_filters(
        &self,
        mut value: HookValue,
        mut args: Vec<HookValue>,
    ) -> Result<HookValue, HookError> {
        let level = {
            let mut state = self.0.borrow_mut();
            if state.callbacks.is_empty() {
                return Ok(value);
            }
            let level = state.nesting_level;
            state.nesting_level += 1;
            let priorities = state.priorities.clone();
            state.iterations.insert(level, Iteration::new(priorities));
            level
        };
        let num_args = args.len() as i64;
        loop {
            let snapshot: Vec<Entry> = {
                let mut state = self.0.borrow_mut();
                let current = state.iterations.get(&level).and_then(Iteration::current);
                state.current_priority.insert(level, current.clone());
                // `$this->callbacks[false]` is `$this->callbacks[0]`.
                let key = current.unwrap_or(ArrayKey::Int(0));
                state
                    .callbacks
                    .get(&key)
                    .map(|slice| slice.values().cloned().collect())
                    .unwrap_or_default()
            };
            for entry in snapshot {
                if !self.0.borrow().doing_action {
                    if args.is_empty() {
                        args.push(value.clone());
                    } else {
                        args[0] = value.clone();
                    }
                }
                value = if entry.accepted_args == 0 {
                    entry.callback.call(&[])?
                } else if entry.accepted_args >= num_args {
                    entry.callback.call(&args)?
                } else {
                    entry.callback.call(slice(&args, entry.accepted_args))?
                };
            }
            let more = self
                .0
                .borrow_mut()
                .iterations
                .get_mut(&level)
                .is_some_and(Iteration::next);
            if !more {
                break;
            }
        }
        let mut state = self.0.borrow_mut();
        state.iterations.remove(&level);
        state.current_priority.remove(&level);
        state.nesting_level -= 1;
        Ok(value)
    }

    pub fn do_action(&self, args: Vec<HookValue>) -> Result<(), HookError> {
        self.0.borrow_mut().doing_action = true;
        self.apply_filters(HookValue::Str(String::new()), args)?;
        let mut state = self.0.borrow_mut();
        if state.nesting_level == 0 {
            state.doing_action = false;
        }
        Ok(())
    }

    /// The `all` hook's run: every callback gets every argument.
    pub fn do_all_hook(&self, args: &[HookValue]) -> Result<(), HookError> {
        let level = {
            let mut state = self.0.borrow_mut();
            let level = state.nesting_level;
            state.nesting_level += 1;
            let priorities = state.priorities.clone();
            state.iterations.insert(level, Iteration::new(priorities));
            level
        };
        loop {
            let snapshot: Vec<Entry> = {
                let state = self.0.borrow();
                let key = state
                    .iterations
                    .get(&level)
                    .and_then(Iteration::current)
                    .unwrap_or(ArrayKey::Int(0));
                state
                    .callbacks
                    .get(&key)
                    .map(|slice| slice.values().cloned().collect())
                    .unwrap_or_default()
            };
            for entry in snapshot {
                entry.callback.call(args)?;
            }
            let more = self
                .0
                .borrow_mut()
                .iterations
                .get_mut(&level)
                .is_some_and(Iteration::next);
            if !more {
                break;
            }
        }
        let mut state = self.0.borrow_mut();
        state.iterations.remove(&level);
        state.nesting_level -= 1;
        Ok(())
    }

    /// The priority the outermost running iteration is at; `None` (PHP's `false`) when none is running.
    pub fn current_priority(&self) -> Option<ArrayKey> {
        self.0
            .borrow()
            .iterations
            .values()
            .next()
            .and_then(Iteration::current)
    }

    /// Drops every callback of `partition`, and the run state (a new request starts with no hook running).
    pub fn drop_partition(&self, partition: Partition) {
        let mut state = self.0.borrow_mut();
        for slice in state.callbacks.values_mut() {
            slice.retain(|_, entry| entry.partition != partition);
        }
        state.callbacks.retain(|_, slice| !slice.is_empty());
        state.priorities = state.callbacks.keys().cloned().collect();
        state.iterations.clear();
        state.current_priority.clear();
        state.nesting_level = 0;
        state.doing_action = false;
    }

    /// `$this->callbacks[$priority] = $slice` (or `[] = $slice` when `priority` is `None`), as `offsetSet()` does.
    pub fn set_priority(&self, priority: Option<ArrayKey>, slice: PrioritySlice) {
        let mut state = self.0.borrow_mut();
        let key = priority.unwrap_or_else(|| {
            let next = state.callbacks.keys().filter_map(|k| match k {
                ArrayKey::Int(n) => Some(*n),
                ArrayKey::Str(_) => None,
            });
            ArrayKey::Int(next.max().map_or(0, |n| n.saturating_add(1)).max(0))
        });
        state.callbacks.insert(key, slice);
        state.priorities = state.callbacks.keys().cloned().collect();
    }

    /// `unset($this->callbacks[$priority])`, as `offsetUnset()` does.
    pub fn unset_priority(&self, priority: &ArrayKey) {
        let mut state = self.0.borrow_mut();
        state.callbacks.shift_remove(priority);
        state.priorities = state.callbacks.keys().cloned().collect();
    }
}

/// `array_slice($args, 0, $length)`.
fn slice(args: &[HookValue], length: i64) -> &[HookValue] {
    let len = args.len() as i64;
    let end = if length < 0 {
        (len + length).max(0)
    } else {
        length.min(len)
    };
    &args[..end as usize]
}
