use std::cell::{Cell, RefCell};
use std::rc::Rc;

use indexmap::IndexMap;
use speedyspec_wp_shortcode_contracts::{ShortcodeHandler, ShortcodeRegistry, ShortcodeTagNames, TagName};

/// `$shortcode_tags` in memory, in PHP array order.
///
/// Every method releases its borrow before returning, so a handler may add or remove tags while it runs.
#[derive(Default)]
pub struct MemoryRegistry {
    tags: RefCell<IndexMap<Vec<u8>, Rc<dyn ShortcodeHandler>>>,
    generation: Cell<u64>,
}

impl MemoryRegistry {
    /// An empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    fn bump(&self) {
        self.generation.set(self.generation.get() + 1);
    }
}

impl ShortcodeTagNames for MemoryRegistry {
    fn tag_names(&self) -> Vec<TagName> {
        self.tags.borrow().keys().map(|k| TagName::from_global(k.clone())).collect()
    }

    fn generation(&self) -> u64 {
        self.generation.get()
    }
}

impl ShortcodeRegistry for MemoryRegistry {
    fn add(&self, tag: TagName, handler: Rc<dyn ShortcodeHandler>) {
        self.tags.borrow_mut().insert(tag.as_bytes().to_vec(), handler);
        self.bump();
    }

    fn remove(&self, tag: &[u8]) {
        self.tags.borrow_mut().shift_remove(tag);
        self.bump();
    }

    fn remove_all(&self) {
        self.tags.borrow_mut().clear();
        self.bump();
    }

    fn exists(&self, tag: &[u8]) -> bool {
        self.tags.borrow().contains_key(tag)
    }

    fn handler(&self, tag: &[u8]) -> Option<Rc<dyn ShortcodeHandler>> {
        self.tags.borrow().get(tag).cloned()
    }
}
