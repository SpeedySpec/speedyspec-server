//! Ports and plain values of the WordPress translations port.
//!
//! The domain (`speedyspec-wp-i18n`) depends on these traits; adapters implement them: file readers and the file
//! probe in `speedyspec-wp-i18n-files`, the per-worker cache in `speedyspec-wp-i18n-memory`, and the ports to code
//! that is not ported (locale sources, request context, deprecations, escaping) in `speedyspec-wp-i18n-php`.

mod ports;
mod values;

pub use ports::*;
pub use values::*;
