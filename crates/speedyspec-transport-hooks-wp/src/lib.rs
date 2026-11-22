//! Transport hooks over WordPress's Hook API.
//!
//! [`WpTransportHooks`] adapts the `TransportHooks` port to `speedyspec_wp_hooks::HookApi`, so a transport's hooks
//! (`rest_pre_dispatch`, `xmlrpc_call`, …) reach the same callbacks PHP plugins add. [`to_hook`] and [`from_hook`]
//! convert between the neutral value and PHP-shaped hook values.
//!
//! Patterns: Adapter.

mod convert;
mod hooks;

pub use convert::{from_hook, to_hook};
pub use hooks::WpTransportHooks;
