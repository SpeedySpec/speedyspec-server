//! WordPress's Shortcode API (`shortcodes.php`) in Rust.
//!
//! - [`MemoryRegistry`] is `$shortcode_tags` for Rust-only use and tests.
//! - [`FancyRegexScanner`] is the reference [`ShortcodeScanner`](speedyspec_wp_shortcode_contracts::ShortcodeScanner).

mod registry;
mod scan;

pub use registry::MemoryRegistry;
pub use scan::FancyRegexScanner;
