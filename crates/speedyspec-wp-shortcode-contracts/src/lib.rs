//! WordPress's Shortcode API contracts: the plain types, the ports and strategies, and WordPress's pattern strings.
//!
//! - [`TagName`], [`TagSet`], [`ParsedAtts`], [`ShortcodeMatch`] and [`MatchGroups`] are plain values.
//! - [`ShortcodeRegistry`] (a Registry), [`ShortcodeHandler`], [`ShortcodeFilters`], [`Diagnostics`],
//!   [`HtmlSplitter`], [`KsesAttributes`] and [`PhpStrings`] are ports.
//! - [`ShortcodeScanner`] and [`AttsParser`] are strategies, so the SIMD scanner, the regex fast path and the
//!   reference regex can be swapped.
//! - [`get_shortcode_regex`] and the other builders return WordPress's exact strings, which plugins and
//!   `wptexturize` rely on.

mod atts;
mod error;
mod matches;
mod ports;
mod regex_source;
mod tag;

pub use atts::{AttKey, AttsParse, ParsedAtts};
pub use error::{ShortcodeError, Thrown};
pub use matches::{MatchGroups, ShortcodeMatch};
pub use ports::{
    AttsParser, ContextGuard, DiagnosticMessage, Diagnostics, HtmlSplitter, KsesAttributes, PhpStrings,
    ShortcodeFilters, ShortcodeHandler, ShortcodeMatches, ShortcodeRegistry, ShortcodeScanner,
    ShortcodeTagNames,
};
pub use regex_source::{get_shortcode_atts_regex, get_shortcode_regex, preg_quote, shortcode_tag_alternation};
pub use tag::{InvalidTagName, TagName, TagSet};

/// A PHP-shaped value, as the Hook API carries it.
pub type PhpValue = speedyspec_hooks::Value;
/// A PHP array key, as the Hook API carries it.
pub use speedyspec_hooks::Key as PhpKey;

/// Wraps PHP string bytes as a [`PhpValue`].
///
/// The Hook API keeps valid UTF-8 as `Str` and everything else as `Bytes`, so both sides of the bridge agree on
/// which variant a string takes.
pub fn php_bytes(bytes: Vec<u8>) -> PhpValue {
    match String::from_utf8(bytes) {
        Ok(s) => PhpValue::Str(s),
        Err(e) => PhpValue::Bytes(e.into_bytes()),
    }
}

/// Borrows the bytes of a string [`PhpValue`].
///
/// Gives `None` for every non-string value, so callers decide how PHP would convert it.
pub fn php_str_bytes(value: &PhpValue) -> Option<&[u8]> {
    match value {
        PhpValue::Str(s) => Some(s.as_bytes()),
        PhpValue::Bytes(b) => Some(b),
        _ => None,
    }
}
