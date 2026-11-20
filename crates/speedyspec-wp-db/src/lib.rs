//! WordPress's `wpdb`, in Rust.
//!
//! [`WpDb`] is the port's API (Gateway) and the handler of every command and query. Pure domain services hold
//! WordPress's rules; `WpdbState` and `WpdbHost` are its outbound ports.

mod config;
mod error;

pub use config::{DbHost, WpDbConfig};
pub use error::WpDbError;
