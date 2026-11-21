//! WordPress's Shortcode API (`shortcodes.php`) in Rust.
//!
//! - [`ShortcodeApi`] is the Facade: one method per WordPress function, over [`ShortcodeDeps`].
//! - [`NoFilters`] is the Null Object filters, for when nothing listens.
//! - [`HandAttsParser`] and [`RegexAttsParser`] are `shortcode_parse_atts()`.
//! - [`MemoryRegistry`] is `$shortcode_tags` for Rust-only use and tests.
//! - [`HandScanner`] is the default, SIMD scanner, and [`candidate_tokens`] is `do_shortcode()`'s tag prefilter.
//! - [`FancyRegexScanner`] is the reference [`ShortcodeScanner`](speedyspec_wp_shortcode_contracts::ShortcodeScanner).

mod api;
mod atts;
mod filters_none;
mod html_tags;
mod registry;
mod strtr;
mod scan;

pub use api::{ShortcodeApi, ShortcodeDeps};
pub use atts::{HandAttsParser, RegexAttsParser};
pub use filters_none::NoFilters;
pub use registry::MemoryRegistry;
pub use scan::{FancyRegexScanner, HandScanner, candidate_tokens};
