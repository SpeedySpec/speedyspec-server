//! In-memory adapters for tests and Rust-only use.

mod resolver;

pub use resolver::{
    MemoryActiveTheme, MemoryGlobalStylesPosts, MemoryVariationRegistrar, MemoryWpCache,
    NoLayerFilters,
};

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
    /// The block styles registry: block, style name, style (`name`, `label`, `style_data`, …).
    styles: RefCell<Vec<(String, String, Value)>>,
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

    /// `register_block_style( $block, $style )`: the style joins the registry, and the block's style names when the
    /// block is registered.
    pub fn register_style(&self, block: &str, style: Value) {
        let name = match crate::array_path::get(&style, &["name"]) {
            Some(Value::Str(s)) => s.clone(),
            _ => return,
        };
        let mut styles = self.styles.borrow_mut();
        match styles.iter_mut().find(|(b, n, _)| b == block && *n == name) {
            Some(slot) => slot.2 = style,
            None => styles.push((block.to_owned(), name.clone(), style)),
        }
        drop(styles);
        let mut blocks = (**self.blocks.borrow()).clone();
        if let Some(found) = blocks.iter_mut().find(|b| b.name == block)
            && !found.style_names.contains(&name)
        {
            found.style_names.push(name);
        }
        *self.blocks.borrow_mut() = Rc::new(blocks);
        self.marker.set(self.marker.get() + 1);
    }

    /// The style names registered for a block (`get_registered_styles_for_block`).
    pub fn styles_for(&self, block: &str) -> Vec<String> {
        self.styles
            .borrow()
            .iter()
            .filter(|(b, _, _)| b == block)
            .map(|(_, n, _)| n.clone())
            .collect()
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

/// A user who can do nothing (no user, as in a request without one).
pub struct NoCapabilities;

impl speedyspec_wp_theme_json_contracts::ports::Capabilities for NoCapabilities {
    fn current_user_can(&self, _capability: &str) -> bool {
        false
    }
}

/// A user with the capabilities listed.
#[derive(Default)]
pub struct MemoryCapabilities {
    /// The capabilities.
    pub capabilities: RefCell<Vec<String>>,
}

impl MemoryCapabilities {
    /// A user with these capabilities.
    pub fn new(capabilities: &[&str]) -> MemoryCapabilities {
        MemoryCapabilities {
            capabilities: RefCell::new(capabilities.iter().map(|c| (*c).to_owned()).collect()),
        }
    }
}

impl speedyspec_wp_theme_json_contracts::ports::Capabilities for MemoryCapabilities {
    fn current_user_can(&self, capability: &str) -> bool {
        self.capabilities.borrow().iter().any(|c| c == capability)
    }
}
