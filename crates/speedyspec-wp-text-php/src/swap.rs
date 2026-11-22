//! Formatting's callbacks added by name, as Rust callbacks.
//!
//! When `default-filters.php` or a plugin calls `add_filter( 'the_content', 'wpautop' )`, the `AddFilter` use case
//! is decorated so the hook store gets a Rust callback in place of the PHP string. It has the same id, the same
//! slot, and `$wp_filter` still shows the same string. It runs the native's body without a PHP call, so a chain of
//! formatting callbacks runs in Rust from `apply_filters` to its result. Every other callback passes through
//! unchanged.
//!
//! A value that isn't a string, or arguments the body rejects, go to the PHP function instead: that gives PHP's own
//! coercion, notice or exception, and the body has done no work yet. A failure inside the work comes back with its
//! payload, so the hooks bridge rethrows the same exception object, or stays silent while PHP unwinds.

use std::collections::HashMap;
use std::rc::Rc;

use speedyspec_container::{Container, Resolved};
use speedyspec_cqrs::{Command, CommandHandler};
use speedyspec_php::Callable;
use speedyspec_php_ext::Value;
use speedyspec_wp_hooks::{AddFilter, Callback, HookCallback, HookError};
use speedyspec_wp_hooks_php::PhpCallable;
use speedyspec_wp_kses_contracts::KsesError;
use speedyspec_wp_text_contracts::TextError;

use crate::args::required;
use crate::native::{Fail, Native};

/// Why a native body could not give a result.
pub enum CallbackFailure {
    /// Its arguments were rejected before any work: calling the PHP function instead gives PHP's exact error.
    Arguments,
    /// The work failed; the hook error carries the original payload.
    Work(HookError),
}

/// A body's call: the hook's arguments to the function's result.
pub type BodyFn = dyn Fn(&[Value]) -> Result<Value, CallbackFailure>;

/// A native function's body, runnable as a hook callback.
#[derive(Clone)]
pub struct NativeBody {
    /// WordPress's name for the function.
    pub name: &'static str,
    /// The body: arity check, then the native's work.
    pub run: Rc<BodyFn>,
}

/// Finds the native body behind a function name added as a callback (port).
pub trait NativeCallbacks {
    /// The body for `name`, when it is one of the functions run as Rust callbacks.
    ///
    /// @param name
    ///     The callable string as given (any case, perhaps with one leading `\`).
    /// @return
    ///     The body, or `None` to keep the PHP callable.
    fn body(&self, name: &str) -> Option<NativeBody>;
}

/// The functions run as Rust callbacks: those WordPress itself adds as formatting callbacks.
pub fn swapped_names() -> Vec<&'static str> {
    let mut names = speedyspec_wp_text::default_callback_names();
    names.push("wp_targeted_link_rel");
    names
}

/// The natives of this crate, by lowercased name, as [`NativeCallbacks`] (adapter).
pub struct NativeTable {
    by_name: HashMap<String, (&'static str, usize, usize, Native)>,
}

impl NativeTable {
    /// The table of [`swapped_names`].
    ///
    /// @return
    ///     The adapter.
    pub fn new() -> NativeTable {
        let names = swapped_names();
        let by_name = crate::native::entries()
            .into_iter()
            .filter(|(name, ..)| names.contains(name))
            .map(|entry| (entry.0.to_ascii_lowercase(), entry))
            .collect();
        NativeTable { by_name }
    }
}

impl Default for NativeTable {
    fn default() -> Self {
        NativeTable::new()
    }
}

impl NativeCallbacks for NativeTable {
    fn body(&self, name: &str) -> Option<NativeBody> {
        let bare = name.strip_prefix('\\').unwrap_or(name);
        let &(name, req, max, f) = self.by_name.get(&bare.to_ascii_lowercase())?;
        let run = move |args: &[Value]| -> Result<Value, CallbackFailure> {
            required(name, args, req, max).map_err(|_| CallbackFailure::Arguments)?;
            f(args).map_err(|e| match e {
                Fail::Native(_) => CallbackFailure::Arguments,
                Fail::Text(e) => CallbackFailure::Work(hook_error(e)),
            })
        };
        Some(NativeBody { name, run: Rc::new(run) })
    }
}

/// A use case's error as a hook error the hooks bridge understands: a kses error is unwrapped to the payload it
/// carries (a PHP exception, or the bridge's unwinding mark).
fn hook_error(e: TextError) -> HookError {
    if let Some(KsesError::Foreign { payload, .. }) = e.payload.as_ref().and_then(|p| p.downcast_ref::<KsesError>()) {
        return HookError { message: e.message, payload: Some(payload.0.clone()) };
    }
    speedyspec_wp_text::text_hook_error(e)
}

/// A function added by name, run as its native body; the PHP function when the body can't take the call.
struct RustCallback {
    body: NativeBody,
    fallback: PhpCallable,
}

impl HookCallback for RustCallback {
    fn call(&self, args: &[Value]) -> Result<Value, HookError> {
        if !matches!(args.first(), Some(Value::Str(_) | Value::Bytes(_))) {
            return self.fallback.call(args);
        }
        match (self.body.run)(args) {
            Ok(value) => Ok(value),
            Err(CallbackFailure::Arguments) => self.fallback.call(args),
            Err(CallbackFailure::Work(e)) => Err(e),
        }
    }
}

/// Decorates the `AddFilter` use case: formatting's functions added by name become Rust callbacks.
pub struct SwapNativeCallbacks {
    inner: Rc<dyn CommandHandler<AddFilter>>,
    natives: Rc<dyn NativeCallbacks>,
}

impl SwapNativeCallbacks {
    /// The decorator around `inner`.
    ///
    /// @param inner
    ///     The handler bound before.
    /// @param natives
    ///     Where the bodies come from.
    /// @return
    ///     The decorator.
    pub fn new(inner: Rc<dyn CommandHandler<AddFilter>>, natives: Rc<dyn NativeCallbacks>) -> SwapNativeCallbacks {
        SwapNativeCallbacks { inner, natives }
    }

    fn swap(&self, callback: Callback) -> Callback {
        let Callback::Foreign { value, id: Some(id), .. } = &callback else {
            return callback;
        };
        let name = match value {
            Value::Str(name) => name.as_str(),
            Value::Bytes(name) => match std::str::from_utf8(name) {
                Ok(name) => name,
                Err(_) => return callback,
            },
            _ => return callback,
        };
        if name.contains("::") {
            return callback;
        }
        let Some(body) = self.natives.body(name) else {
            return callback;
        };
        let fallback = PhpCallable(Callable::new(value.clone()));
        Callback::Rust { id: id.clone(), run: Rc::new(RustCallback { body, fallback }) }
    }
}

impl CommandHandler<AddFilter> for SwapNativeCallbacks {
    fn handle(&self, mut m: AddFilter) -> Result<bool, HookError> {
        m.callback = self.swap(m.callback);
        self.inner.handle(m)
    }
}

/// Decorates whatever handles `AddFilter` in `container`, now and after any rebinding. A handler bound from PHP
/// (not a Rust trait object) is left as it is.
///
/// @param container
///     Where the Hook API's use cases are bound.
/// @param natives
///     Where the bodies come from.
pub fn install(container: &Container, natives: Rc<dyn NativeCallbacks>) {
    container.extend(AddFilter::KEY, move |resolved: Resolved, _| {
        let Some(inner) = resolved.downcast_ref::<Rc<dyn CommandHandler<AddFilter>>>() else {
            return Ok(resolved);
        };
        let decorated: Rc<dyn CommandHandler<AddFilter>> = Rc::new(SwapNativeCallbacks::new(inner.clone(), natives.clone()));
        Ok(Rc::new(decorated) as Resolved)
    });
}
