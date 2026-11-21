//! Outbound ports this crate adds to the contracts' ones.

use speedyspec_hooks::Value as HookValue;
use speedyspec_wp_kses_contracts::{KsesError, ValueCallback};

/// The PHP callables kses refers to: a `value_callback` left in an allowlist, and the PHP functions it registers on
/// filters by name (`wp_filter_global_styles_post`, which stays PHP until the theme port).
pub trait ValueCallbacks {
    fn resolve(&self, callable: &HookValue) -> Option<ValueCallback>;
    /// A PHP function as a filter callback; `None` where there is no PHP (Rust callers), and kses then registers
    /// nothing for it.
    fn named_filter(&self, _name: &str) -> Option<speedyspec_wp_hooks::Callback> {
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
