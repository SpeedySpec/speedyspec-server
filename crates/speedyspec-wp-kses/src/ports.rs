//! Outbound ports this crate adds to the contracts' ones.

use speedyspec_hooks::Value as HookValue;
use speedyspec_wp_kses_contracts::{KsesError, ValueCallback};

/// Turns a PHP callable left in an allowlist's `value_callback` into a callback; `None` when it cannot.
pub trait ValueCallbacks {
    fn resolve(&self, callable: &HookValue) -> Option<ValueCallback>;
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
