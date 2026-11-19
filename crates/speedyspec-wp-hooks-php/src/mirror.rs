//! `$wp_current_filter` is a real PHP array, kept equal to the Rust store's current-hook stack wherever PHP can see
//! it: on every entry from PHP into a hook function the array is read back (PHP may have appended to it, popped it
//! or replaced it), and before control returns to PHP (a native function's return, a PHP callback's call) the
//! store's stack is written to it. A Rust caller with Rust callbacks never touches it.

use std::cell::RefCell;

use speedyspec_php_ext::{Key, Value};
use speedyspec_wp_hooks::HookStore;

const GLOBAL: &str = "wp_current_filter";

thread_local! {
    /// What the PHP array held when it was last read or written here; `None` outside a request.
    static MIRRORED: RefCell<Option<Vec<Value>>> = const { RefCell::new(None) };
}

/// Starts a request: the global is the (empty) array PHP just installed.
pub fn start() {
    MIRRORED.with(|m| *m.borrow_mut() = Some(Vec::new()));
}

pub fn end() {
    MIRRORED.with(|m| *m.borrow_mut() = None);
}

fn active() -> bool {
    MIRRORED.with(|m| m.borrow().is_some())
}

/// PHP's changes to the array, into the store.
pub fn sync_in(store: &dyn HookStore) {
    if !active() {
        return;
    }
    let Some(Value::Array(entries)) = speedyspec_php::global(GLOBAL) else {
        return;
    };
    let values: Vec<Value> = entries.into_iter().map(|(_, v)| v).collect();
    let changed = MIRRORED.with(|m| m.borrow().as_ref() != Some(&values));
    if changed {
        store.set_current_stack(values.clone());
        MIRRORED.with(|m| *m.borrow_mut() = Some(values));
    }
}

/// The store's stack, into the array.
pub fn sync_out(store: &dyn HookStore) {
    if !active() {
        return;
    }
    let stack = store.current_stack();
    let changed = MIRRORED.with(|m| m.borrow().as_ref() != Some(&stack));
    if changed {
        let list = stack
            .iter()
            .enumerate()
            .map(|(i, v)| (Key::Int(i as i64), v.clone()))
            .collect();
        if speedyspec_php::set_global(GLOBAL, &Value::Array(list)).is_ok() {
            MIRRORED.with(|m| *m.borrow_mut() = Some(stack));
        }
    }
}
