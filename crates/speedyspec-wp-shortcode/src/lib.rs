//! WordPress's Shortcode API (`shortcodes.php`) in Rust.
//!
//! - [`MemoryRegistry`] is `$shortcode_tags` for Rust-only use and tests.

mod registry;

pub use registry::MemoryRegistry;
