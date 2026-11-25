//! WordPress's kses sanitizer in Rust.
//!
//! - [`KsesApi`] is the facade: `wp_kses()` and the functions around it, firing kses's filters through the Rust
//!   Hook API exactly as WordPress fires them.
//! - The scanner is fused: one pass tokenizes, checks attributes and writes the output, built from the matchers in
//!   `speedyspec-wp-kses-lexical`, so it follows the pinned WordPress release's kses.php byte for byte.
//! - The default allowlists are shared tables (Flyweight), read once per thread.

mod api;
mod convert;
mod defaults;
mod functions;
mod globals;
mod init;
mod messages;
mod ports;
mod ports_impl;
mod safecss;
mod scanner;

pub use api::{KsesApi, KsesDeps};
pub use convert::{
    allowed_from_value, allowed_spec_to_value, allowed_to_value, attr_rule, attr_rules_from_value,
    rule_value,
};
pub use defaults::{
    add_global_attributes, allow_pdf_objects, default_post_value, default_tags_value,
};
pub use functions::{addslashes, allow_note_mention_span, php_stripslashes};
pub use globals::{AllowedGlobals, DefaultGlobals};
pub use messages::*;
pub use ports::{
    MemoryOperatingMode, NoHtmlApiSanitizer, NoValueCallbacks, UploadsLocation, ValueCallbacks,
};
pub use scanner::{
    CheckedAttr, directive_binds_value_restricted_attribute, restricts_attribute_values,
};
