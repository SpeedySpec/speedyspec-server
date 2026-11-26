//! Outbound ports this crate adds to the contracts' ones.

use std::any::Any;
use std::cell::RefCell;

use speedyspec_hooks::Value as HookValue;
use speedyspec_wp_kses_contracts::{
    AllowedSpec, HtmlApiSanitizer, KsesError, OperatingMode, Protocols, ValueCallback,
};

/// The PHP callables kses refers to: a `value_callback` left in an allowlist, and the PHP functions it registers on
/// filters by name (`wp_filter_global_styles_post`, which stays PHP until the theme port).
pub trait ValueCallbacks {
    fn resolve(&self, callable: &HookValue) -> Option<ValueCallback>;
    /// A PHP function as a filter callback; `None` where there is no PHP (Rust callers), and kses then registers
    /// nothing for it.
    fn named_filter(&self, _name: &str) -> Option<speedyspec_wp_hooks_contracts::Callback> {
        None
    }
}

/// No callables beyond the ones kses ports natively.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoValueCallbacks;

impl ValueCallbacks for NoValueCallbacks {
    fn resolve(&self, _callable: &HookValue) -> Option<ValueCallback> {
        None
    }
}

/// Where uploads are served from (`wp_upload_dir()['url']`), for `_wp_kses_allow_pdf_objects()`.
pub trait UploadsLocation {
    fn upload_url(&self) -> Result<Vec<u8>, KsesError>;
}

/// No HTML API implementation: where nothing provides `wp_sanitize_html_kses()` (Rust callers with no PHP), the
/// legacy parser runs whatever `wp_kses_force_legacy_parser` answers.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoHtmlApiSanitizer;

impl HtmlApiSanitizer for NoHtmlApiSanitizer {
    fn sanitize(
        &self,
        _content: &[u8],
        _allowed: &AllowedSpec,
        _protocols: Option<&Protocols>,
    ) -> Result<Option<Vec<u8>>, KsesError> {
        Ok(None)
    }
}

/// `$wp_kses_operating_mode` held in memory, `legacy` at first as kses.php sets it.
#[derive(Debug)]
pub struct MemoryOperatingMode(RefCell<HookValue>);

impl Default for MemoryOperatingMode {
    fn default() -> MemoryOperatingMode {
        MemoryOperatingMode(RefCell::new(HookValue::Str("legacy".into())))
    }
}

impl MemoryOperatingMode {
    /// The mode as it is now.
    pub fn get(&self) -> HookValue {
        self.0.borrow().clone()
    }
}

impl OperatingMode for MemoryOperatingMode {
    fn enter(&self, mode: &'static str) -> Box<dyn Any> {
        Box::new(self.0.replace(HookValue::Str(mode.into())))
    }

    fn restore(&self, previous: Box<dyn Any>) {
        if let Ok(previous) = previous.downcast::<HookValue>() {
            *self.0.borrow_mut() = *previous;
        }
    }
}
