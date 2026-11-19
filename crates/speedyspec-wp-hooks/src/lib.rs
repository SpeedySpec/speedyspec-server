//! WordPress's Hook API, in Rust.
//!
//! - [`HookApi`] is `plugin.php`: every function, over the outbound port [`HookStore`] (`$wp_filter`, `$wp_actions`,
//!   `$wp_filters`, `$wp_current_filter`).
//! - [`WpHook`] is `WP_Hook`, ported literally, with [`HookIteration`] (how running iterations follow changes) and
//!   [`PriorityOrder`] (PHP's key ordering and comparisons).
//! - [`CallbackIdentity`] is `_wp_filter_build_unique_id()`.
//! - Each function is a use case (`messages`): a command or query whose handler is [`HookApi`], bound in the service
//!   container by [`register_hook_handlers`].

mod api;
mod callback;
mod hook;
mod key;
mod messages;
mod store;

pub use api::{DeprecatedHooks, HookApi, IgnoreDeprecations};
pub use callback::{
    Callback, CallbackId, CallbackIdentity, CallbackShape, HookCallback, HookError,
};
pub use hook::{
    Entry, HasFilterResult, HookIteration, HookState, Partition, PrioritySlice, WpHook,
};
pub use key::{
    ArrayKey, HookKey, HookName, Prehashed, PrehashedHasher, PriorityOrder, php_string_hash,
};
pub use messages::*;
pub use store::{Count, HookStore};

/// A value passed through hooks, shaped like a PHP value.
pub type HookValue = speedyspec_hooks::Value;
pub use speedyspec_hooks::Key;
