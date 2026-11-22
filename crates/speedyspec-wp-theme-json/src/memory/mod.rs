//! In-memory adapters for tests and Rust-only use.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use speedyspec_hooks::Value;

use speedyspec_wp_theme_json_contracts::ports::{
    BlockMetadata, Diagnostics, Gettext, RegisteredBlock,
};

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

    fn current(&self, name: &str) -> Option<RegisteredBlock> {
        self.blocks
            .borrow()
            .iter()
            .find(|b| b.name == name)
            .cloned()
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
    /// One line per notice, as the compat stubs write them: `trigger_error:<function>|<message>`,
    /// `deprecated_function:<function>|<version>|<replacement>`, `doing_it_wrong:<function>|<message>|<version>`,
    /// `deprecated_argument:<function>|<version>|<message>`.
    pub notices: RefCell<Vec<String>>,
}

impl Diagnostics for RecordedDiagnostics {
    fn trigger_error(&self, function: &str, message: &[u8]) {
        self.notices.borrow_mut().push(format!(
            "trigger_error:{function}|{}",
            String::from_utf8_lossy(message)
        ));
    }

    fn deprecated_function(&self, function: &str, version: &str, replacement: &str) {
        self.notices.borrow_mut().push(format!(
            "deprecated_function:{function}|{version}|{replacement}"
        ));
    }

    fn doing_it_wrong(&self, function: &str, message: &[u8], version: &str) {
        self.notices.borrow_mut().push(format!(
            "doing_it_wrong:{function}|{}|{version}",
            String::from_utf8_lossy(message)
        ));
    }

    fn deprecated_argument(&self, function: &str, version: &str, message: &[u8]) {
        self.notices.borrow_mut().push(format!(
            "deprecated_argument:{function}|{version}|{}",
            String::from_utf8_lossy(message)
        ));
    }
}

/// The site's settings, held in memory.
pub struct MemoryGlobalSettings {
    /// What `wp_get_global_settings()` returns.
    pub settings: RefCell<Value>,
}

impl Default for MemoryGlobalSettings {
    fn default() -> Self {
        MemoryGlobalSettings::new(Value::Array(Vec::new()))
    }
}

impl MemoryGlobalSettings {
    /// These settings.
    pub fn new(settings: Value) -> MemoryGlobalSettings {
        MemoryGlobalSettings {
            settings: RefCell::new(settings),
        }
    }
}

impl speedyspec_wp_theme_json_contracts::ports::GlobalSettings for MemoryGlobalSettings {
    fn settings(&self) -> Value {
        match &*self.settings.borrow() {
            Value::Null => Value::Array(Vec::new()),
            other => other.clone(),
        }
    }
}

/// A theme that supports no optional features.
pub struct NoThemeSupports;

impl speedyspec_wp_theme_json_contracts::ports::ActiveTheme for NoThemeSupports {
    fn supports(&self, _feature: &str) -> bool {
        false
    }
}

/// A theme that supports the features listed.
#[derive(Default)]
pub struct MemoryThemeSupports {
    /// The supported features.
    pub features: RefCell<Vec<String>>,
}

impl speedyspec_wp_theme_json_contracts::ports::ActiveTheme for MemoryThemeSupports {
    fn supports(&self, feature: &str) -> bool {
        self.features.borrow().iter().any(|f| f == feature)
    }
}
