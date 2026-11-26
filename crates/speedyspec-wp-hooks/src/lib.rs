//! WordPress's Hook API, in Rust.
//!
//! - [`WpHookApi`] is `plugin.php`: every function, over the outbound port [`HookStore`] (`$wp_filter`, `$wp_actions`,
//!   `$wp_filters`, `$wp_current_filter`).
//! - [`WpHook`] is `WP_Hook`, ported literally, with [`HookIteration`] (how running iterations follow changes) and
//!   [`PriorityOrder`] (PHP's key ordering and comparisons).
//! - [`CallbackIdentity`] is `_wp_filter_build_unique_id()`.
//! - Each function is a use case (`messages`): a command or query whose handler is [`WpHookApi`], bound in the service
//!   container by [`register_hook_handlers`].

mod api;
mod callback;
mod hook;
mod key;
mod messages;
mod store;

pub use api::{DeprecatedHooks, WpHookApi, IgnoreDeprecations};
pub use callback::{CallbackIdentity, CallbackShape};
pub use hook::{Entry, HookIteration, HookState, Partition, PrioritySlice, WpHook};
pub use key::PriorityOrder;
pub use messages::*;
pub use store::HookStore;
