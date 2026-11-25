//! kses's `AttributeEscaper` over `EscapeApi::esc_attr`, so `attribute_escape` fires in exactly one place.

use std::rc::Rc;

use speedyspec_wp_kses_contracts::{AttributeEscaper, KsesError};
use speedyspec_wp_text_escape::EscapeApi;

use crate::strings::to_php_string;

/// `esc_attr()` for kses (Adapter): kses escapes attribute values through this port.
pub struct EscapeAttributes(pub Rc<EscapeApi>);

impl AttributeEscaper for EscapeAttributes {
    fn esc_attr(&self, value: &[u8]) -> Result<Vec<u8>, KsesError> {
        let escaped = self
            .0
            .esc_attr(value)
            .map_err(|e| KsesError::Callback(e.message))?;
        // A filter may return a non-string; kses concatenates it, as PHP would.
        Ok(to_php_string(&escaped).unwrap_or_default())
    }
}

/// `current_user_can( 'unfiltered_html' )` answering no, for Rust callers without a user.
pub struct NoUnfilteredHtml;

impl speedyspec_wp_kses_contracts::UnfilteredHtmlPolicy for NoUnfilteredHtml {
    fn can_unfiltered_html(&self) -> Result<bool, KsesError> {
        Ok(false)
    }
}

/// A fixed uploads URL for kses's `wp_kses_uri_attributes` checks.
pub struct FixedUploads(pub Vec<u8>);

impl speedyspec_wp_kses::UploadsLocation for FixedUploads {
    fn upload_url(&self) -> Result<Vec<u8>, KsesError> {
        Ok(self.0.clone())
    }
}

/// A kses facade for formatting: attribute values escaped through `escape`, no unfiltered HTML, WordPress's
/// default allowlists and entity names, no value callbacks.
///
/// @param hooks
///     The hooks kses fires its filters through.
/// @param escape
///     The escapers.
/// @return
///     The facade.
pub fn kses_for(
    hooks: Rc<speedyspec_wp_hooks::HookApi>,
    escape: Rc<EscapeApi>,
) -> Rc<speedyspec_wp_kses::KsesApi> {
    speedyspec_wp_kses::KsesApi::shared(speedyspec_wp_kses::KsesDeps {
        hooks,
        escaper: Rc::new(EscapeAttributes(escape)),
        policy: Rc::new(NoUnfilteredHtml),
        globals: Rc::new(speedyspec_wp_kses::DefaultGlobals::default()),
        uploads: Rc::new(FixedUploads(Vec::new())),
        callbacks: Rc::new(speedyspec_wp_kses::NoValueCallbacks),
        html_api: Rc::new(speedyspec_wp_kses::NoHtmlApiSanitizer),
        mode: Rc::new(speedyspec_wp_kses::MemoryOperatingMode::default()),
    })
}
