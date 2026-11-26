//! The hooks seam's contracts: the Hook API port other seams call, and the values they pass through it.
//!
//! - [`HookApi`] is `plugin.php` as other seams use it. `speedyspec-wp-hooks` implements it.
//! - [`HookName`], [`HookKey`] and [`ArrayKey`] are hook names and priorities as PHP array keys, hashed as PHP does.
//! - [`Callback`], [`CallbackId`] and [`HookError`] are what a hook runs, how it is found again, and how it fails.

mod api;
mod callback;
mod name;

pub use api::{Count, HasFilterResult, HookApi};
pub use callback::{Callback, CallbackId, HookCallback, HookError};
pub use name::{ArrayKey, HookKey, HookName, Prehashed, PrehashedHasher, php_string_hash};

/// A value passed through hooks, shaped like a PHP value.
pub type HookValue = speedyspec_hooks::Value;
pub use speedyspec_hooks::Key;
