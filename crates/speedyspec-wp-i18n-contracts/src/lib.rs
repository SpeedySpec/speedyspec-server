//! Ports and plain values of the WordPress translations port.
//!
//! The domain (`speedyspec-wp-i18n`) depends on these traits; adapters implement them: file readers and the file
//! probe in `speedyspec-wp-i18n-files`, the per-worker cache in `speedyspec-wp-i18n-memory`, and the locale's ports
//! (strings, switching effects, script sources) in the domain's facade or in `speedyspec-wp-i18n-php`.

mod locale;
mod ports;
mod values;

pub use locale::*;
pub use ports::*;
pub use values::*;
