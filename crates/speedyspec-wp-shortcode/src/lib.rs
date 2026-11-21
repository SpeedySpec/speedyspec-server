//! WordPress's Shortcode API (`shortcodes.php`) in Rust.
//!
//! - [`HandAttsParser`] and [`RegexAttsParser`] are `shortcode_parse_atts()`.
//! - [`MemoryRegistry`] is `$shortcode_tags` for Rust-only use and tests.
//! - [`HandScanner`] is the default, SIMD scanner, and [`candidate_tokens`] is `do_shortcode()`'s tag prefilter.
//! - [`FancyRegexScanner`] is the reference [`ShortcodeScanner`](speedyspec_wp_shortcode_contracts::ShortcodeScanner).

mod atts;
mod registry;
mod scan;

pub use atts::{HandAttsParser, RegexAttsParser};
pub use registry::MemoryRegistry;
pub use scan::{FancyRegexScanner, HandScanner, candidate_tokens};
