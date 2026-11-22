//! In-memory adapters for tests and Rust-only use.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use speedyspec_wp_theme_json_contracts::ports::{BlockMetadata, Diagnostics, Gettext, RegisteredBlock};

/// Block metadata held in memory.
#[derive(Default)]
pub struct MemoryBlockMetadata {
    blocks: RefCell<Rc<Vec<RegisteredBlock>>>,
    marker: Cell<u64>,
}

impl MemoryBlockMetadata {
    /// No blocks.
    pub fn new() -> MemoryBlockMetadata {
        MemoryBlockMetadata::default()
    }

    /// Registers a block, replacing any of the same name in place.
    pub fn register(&self, block: RegisteredBlock) {
        let mut blocks = (**self.blocks.borrow()).clone();
        match blocks.iter_mut().find(|b| b.name == block.name) {
            Some(slot) => *slot = block,
            None => blocks.push(block),
        }
        *self.blocks.borrow_mut() = Rc::new(blocks);
        self.marker.set(self.marker.get() + 1);
    }
}

impl BlockMetadata for MemoryBlockMetadata {
    fn registered(&self) -> Rc<Vec<RegisteredBlock>> {
        self.blocks.borrow().clone()
    }

    fn change_marker(&self) -> u64 {
        self.marker.get()
    }
}

/// Translations that return the text unchanged, as an English site does.
pub struct NoTranslation;

impl Gettext for NoTranslation {
    fn translate(&self, text: &str, _domain: &str) -> Vec<u8> {
        text.as_bytes().to_vec()
    }
}

/// Diagnostics kept in memory, in the order they were raised.
#[derive(Default)]
pub struct RecordedDiagnostics {
    /// One line per notice: `trigger_error:<function>|<message>` or `deprecated_function:<function>|<version>|<replacement>`.
    pub notices: RefCell<Vec<String>>,
}

impl Diagnostics for RecordedDiagnostics {
    fn trigger_error(&self, function: &str, message: &[u8]) {
        self.notices.borrow_mut().push(format!("trigger_error:{function}|{}", String::from_utf8_lossy(message)));
    }

    fn deprecated_function(&self, function: &str, version: &str, replacement: &str) {
        self.notices.borrow_mut().push(format!("deprecated_function:{function}|{version}|{replacement}"));
    }
}
