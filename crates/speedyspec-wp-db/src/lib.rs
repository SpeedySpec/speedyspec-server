//! WordPress's `wpdb`, in Rust.
//!
//! [`WpDb`] is the port's API (Gateway) and the handler of every command and query. Pure domain services hold
//! WordPress's rules; `WpdbState` and `WpdbHost` are its outbound ports.

mod config;
mod error;

pub use config::{DbHost, WpDbConfig};
pub use error::WpDbError;

mod escape;
mod php;
mod prepare;

pub use escape::{PlaceholderEscape, esc_like, escape, quote_identifier, real_escape, weak_escape};
pub use php::float_to_string;
pub use prepare::{DoingItWrong, Prepare, PrepareOutcome};

/// A PHP value, as the Hook API carries it.
pub type WpValue = speedyspec_hooks::Value;
