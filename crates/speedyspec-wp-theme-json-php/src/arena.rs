//! The documents PHP objects point at, for one request.

use std::cell::RefCell;
use std::rc::Rc;

use speedyspec_wp_theme_json_contracts::model::ThemeJson;

thread_local! {
    static SLOTS: RefCell<Vec<Option<Rc<ThemeJson>>>> = const { RefCell::new(Vec::new()) };
    static FREE: RefCell<Vec<usize>> = const { RefCell::new(Vec::new()) };
}

/// Each `WP_Theme_JSON` object holds a handle into this arena; the arena empties at the end of every request.
pub struct HandleArena;

impl HandleArena {
    /// Stores a document and returns its handle.
    pub fn insert(doc: ThemeJson) -> i64 {
        let doc = Some(Rc::new(doc));
        let slot = FREE.with(|f| f.borrow_mut().pop());
        SLOTS.with(|s| {
            let mut slots = s.borrow_mut();
            match slot {
                Some(i) => {
                    slots[i] = doc;
                    i as i64
                }
                None => {
                    slots.push(doc);
                    (slots.len() - 1) as i64
                }
            }
        })
    }

    /// The document behind a handle.
    pub fn get(handle: i64) -> Option<Rc<ThemeJson>> {
        SLOTS.with(|s| usize::try_from(handle).ok().and_then(|i| s.borrow().get(i).cloned().flatten()))
    }

    /// Replaces the document behind a handle.
    pub fn replace(handle: i64, doc: ThemeJson) {
        SLOTS.with(|s| {
            if let Some(slot) = usize::try_from(handle).ok().and_then(|i| s.borrow_mut().get_mut(i).map(|_| i)) {
                s.borrow_mut()[slot] = Some(Rc::new(doc));
            }
        });
    }

    /// A new handle with a copy of the document, for `clone`.
    pub fn duplicate(handle: i64) -> i64 {
        match Self::get(handle) {
            Some(doc) => Self::insert((*doc).clone()),
            None => -1,
        }
    }

    /// Frees a handle.
    pub fn release(handle: i64) {
        let Ok(i) = usize::try_from(handle) else { return };
        let freed = SLOTS.with(|s| s.borrow_mut().get_mut(i).and_then(Option::take).is_some());
        if freed {
            FREE.with(|f| f.borrow_mut().push(i));
        }
    }

    /// Empties the arena (end of request).
    pub fn clear() {
        SLOTS.with(|s| s.borrow_mut().clear());
        FREE.with(|f| f.borrow_mut().clear());
    }
}
