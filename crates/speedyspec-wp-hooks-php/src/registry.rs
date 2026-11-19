//! The PHP `WP_Hook` objects of this request, and the Rust hooks they are handles to.

use std::cell::RefCell;
use std::collections::HashMap;

use speedyspec_php_ext::Value;
use speedyspec_wp_hooks::WpHook;

#[derive(Default)]
struct Table {
    hooks: Vec<WpHook>,
    objects: HashMap<usize, Value>,
    by_object: HashMap<u64, usize>,
    by_hook: HashMap<usize, usize>,
}

thread_local! {
    static TABLE: RefCell<Table> = RefCell::new(Table::default());
}

/// Registers a PHP object made by `new WP_Hook()`; its Rust hook is new.
pub fn register_new(object: Value) -> usize {
    let hook = WpHook::new();
    TABLE.with(|t| {
        let mut t = t.borrow_mut();
        let id = t.hooks.len();
        t.by_hook.insert(hook.addr(), id);
        t.hooks.push(hook);
        if let Some(object_id) = speedyspec_php::object_id(&object) {
            t.by_object.insert(object_id, id);
        }
        t.objects.insert(id, object);
        id
    })
}

pub fn hook(id: usize) -> Option<WpHook> {
    TABLE.with(|t| t.borrow().hooks.get(id).cloned())
}

/// The Rust hook behind a PHP `WP_Hook` object.
pub fn hook_of(object: &Value) -> Option<WpHook> {
    let object_id = speedyspec_php::object_id(object)?;
    TABLE.with(|t| {
        let t = t.borrow();
        t.by_object
            .get(&object_id)
            .and_then(|id| t.hooks.get(*id).cloned())
    })
}

/// The PHP object for a Rust hook, made (`WP_Hook::speedyspec_wrap`) the first time it is asked for in a request.
pub fn object_for(hook: &WpHook) -> Result<Value, speedyspec_php::PhpError> {
    let existing = TABLE.with(|t| {
        let t = t.borrow();
        t.by_hook
            .get(&hook.addr())
            .and_then(|id| t.objects.get(id).cloned().map(Ok).or(Some(Err(*id))))
    });
    let id = match existing {
        Some(Ok(object)) => return Ok(object),
        Some(Err(id)) => id,
        None => TABLE.with(|t| {
            let mut t = t.borrow_mut();
            let id = t.hooks.len();
            t.by_hook.insert(hook.addr(), id);
            t.hooks.push(hook.clone());
            id
        }),
    };
    let object = speedyspec_php::call(
        &Value::Str("WP_Hook::speedyspec_wrap".into()),
        &[Value::Int(id as i64)],
    )?;
    TABLE.with(|t| {
        let mut t = t.borrow_mut();
        if let Some(object_id) = speedyspec_php::object_id(&object) {
            t.by_object.insert(object_id, id);
        }
        t.objects.insert(id, object.clone());
    });
    Ok(object)
}

/// Forgets every PHP object (they end with the request); the Rust hooks stay in their store.
pub fn clear() {
    let table = TABLE.with(|t| std::mem::take(&mut *t.borrow_mut()));
    drop(table);
}
