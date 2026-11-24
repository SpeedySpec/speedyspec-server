//! The `WP_Ability` objects of the request, by handle: what the Abilities server passes to hooks.

use std::cell::RefCell;

use speedyspec_php_ext::Value as PhpValue;

thread_local! {
    static OBJECTS: RefCell<Vec<PhpValue>> = const { RefCell::new(Vec::new()) };
}

/// Keeps `object` for the request; its handle.
pub fn add(object: PhpValue) -> u64 {
    OBJECTS.with(|o| {
        let mut objects = o.borrow_mut();
        objects.push(object);
        (objects.len() - 1) as u64
    })
}

/// The object a handle stands for, or `null`.
pub fn get(handle: u64) -> PhpValue {
    OBJECTS.with(|o| {
        o.borrow()
            .get(handle as usize)
            .cloned()
            .unwrap_or(PhpValue::Null)
    })
}

/// Forgets the request's objects.
pub fn clear() {
    OBJECTS.with(|o| o.borrow_mut().clear());
}
