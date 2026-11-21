//! WordPress's kses sanitizer in Rust.
//!
//! - [`KsesApi`] is the facade: `wp_kses()` and the functions around it, firing kses's filters through the Rust
//!   Hook API exactly as WordPress fires them.
//! - The scanner is fused: one pass tokenizes, checks attributes and writes the output, built from the matchers in
//!   `speedyspec-wp-kses-lexical`, so it follows WordPress 7.1.5's kses.php byte for byte.
//! - The default allowlists are shared tables (Flyweight), read once per thread.

mod api;
mod convert;
mod defaults;
mod globals;
mod ports;
mod safecss;

pub use api::{KsesApi, KsesDeps};
pub use convert::{allowed_from_value, allowed_to_value, rule_value};
pub use defaults::{
    add_global_attributes, allow_pdf_objects, default_post_value, default_tags_value,
};
pub use globals::{AllowedGlobals, DefaultGlobals};
pub use ports::{NoValueCallbacks, UploadsLocation, ValueCallbacks};
