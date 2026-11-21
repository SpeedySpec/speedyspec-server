//! Ports and plain values of the WordPress translations port.
//!
//! The domain (`speedyspec-wp-i18n`) depends on these traits; adapters implement them: file readers and the file
//! probe in `speedyspec-wp-i18n-files`, and the per-worker cache in `speedyspec-wp-i18n-memory`.

mod ports;
mod values;

pub use ports::*;
pub use values::*;
