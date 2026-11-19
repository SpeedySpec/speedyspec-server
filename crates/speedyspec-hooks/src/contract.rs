use std::fmt;
use std::rc::Rc;

use crate::Value;

/// A hook callback. Rust and PHP callbacks both take this shape; the PHP bridge
/// wraps a PHP callable in one.
pub type Callback = dyn Fn(&[Value]) -> Result<Value, CallbackError>;

/// Returned by a callback to abort the firing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CallbackError(pub String);

/// Options for [`Hooks::add`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AddOptions {
    /// Lower runs first. Default 10, as in WordPress. 64 bits, so PHP's
    /// `PHP_INT_MIN` and `PHP_INT_MAX` (the "run first"/"run last" idioms) fit.
    pub priority: i64,
    /// How many arguments the callback sees. Default: all of them.
    pub accepted_args: usize,
    /// Caller-chosen identity, so a callback can be removed without keeping its
    /// [`CallbackId`] (PHP closures have no stable identity to look up).
    pub key: Option<String>,
    /// Who registered it, named in errors so a failure points at a plugin.
    pub owner: Option<String>,
}

impl Default for AddOptions {
    fn default() -> Self {
        AddOptions {
            priority: 10,
            accepted_args: usize::MAX,
            key: None,
            owner: None,
        }
    }
}

/// Identifies one registration within one backend instance.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CallbackId(pub u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HookErrorKind {
    /// A callback returned an error.
    Callback,
    /// The backend has to move a value out of the process and it holds an `Opaque`.
    NotPortable,
    /// The backend itself failed (connection, timeout, ...).
    Backend,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HookError {
    pub kind: HookErrorKind,
    pub hook: String,
    /// The owner of the callback that failed, when `kind` is `Callback`.
    pub owner: Option<String>,
    pub message: String,
}

impl fmt::Display for HookError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "hook '{}'", self.hook)?;
        if let Some(owner) = &self.owner {
            write!(f, " (registered by {owner})")?;
        }
        write!(f, ": {}", self.message)
    }
}

impl std::error::Error for HookError {}

/// A hook store.
///
/// Every method is fallible and takes `&self`: a broker backend can fail on any
/// of them and may be shared. The trait is synchronous; a broker backend blocks
/// inside the call.
///
/// Semantics every backend must have: lower priority runs first, insertion order
/// within a priority. `run` passes `args` and ignores return values. `filter`
/// passes `[value, args...]` and each return value becomes the next callback's
/// `value`. Both truncate what a callback sees to its `accepted_args`. `ran`
/// counts `run` calls for a name whether or not anything is registered. The
/// first callback error stops the firing and is returned with kind `Callback`
/// and that callback's `owner`. A callback may add, remove or fire hooks,
/// including the one running; such changes apply from the next firing.
pub trait Hooks {
    fn add(
        &self,
        hook: &str,
        callback: Rc<Callback>,
        options: AddOptions,
    ) -> Result<CallbackId, HookError>;
    /// Ok(true) if something was removed.
    fn remove(&self, hook: &str, id: CallbackId) -> Result<bool, HookError>;
    /// Removes by [`AddOptions::key`]; the priority must match the one it was added at.
    fn remove_keyed(&self, hook: &str, key: &str, priority: i64) -> Result<bool, HookError>;
    fn has(&self, hook: &str) -> Result<bool, HookError>;
    fn has_keyed(&self, hook: &str, key: &str) -> Result<bool, HookError>;
    fn run(&self, hook: &str, args: &[Value]) -> Result<(), HookError>;
    fn filter(&self, hook: &str, value: Value, args: &[Value]) -> Result<Value, HookError>;
    fn ran(&self, hook: &str) -> Result<u64, HookError>;
}

/// What every consumer holds.
pub type SharedHooks = Rc<dyn Hooks>;
