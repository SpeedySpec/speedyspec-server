//! Embedded, statically linked PHP for speedyspec.
//!
//! [`Php::boot`] starts the interpreter once per process. PHP is
//! process-global and non-thread-safe, so a [`Php`] stays on the thread that
//! booted it. The hook registry passed in [`BootOptions`] is exposed to PHP as
//! `SpeedySpec\Hooks\Hook` and the WordPress-named functions.

mod area;
mod boundary;
mod convert;
mod hook_class;
pub mod native;
mod runtime;
mod sapi;
mod web;

pub use area::NativeArea;
/// ext-php-rs, for areas building their functions and classes.
pub use ext_php_rs;
pub use runtime::{
    BootOptions, CallFailure, Output, Php, PhpError, Served, call, call_catching, eval, global,
    object_id, set_global, set_ini,
};
pub use web::WebRequest;

/// The PHP version linked into this binary, from the repository's
/// `php-version` file.
pub const PHP_VERSION: &str = env!("SPEEDYSPEC_PHP_VERSION");

/// Identifies the execution context PHP is running in now: the main context or one fiber
/// (the address of its `zend_fiber_context`, `EG(current_fiber_context)`), 0 when PHP is not
/// running. Within one context calls nest strictly, also while other contexts are suspended
/// mid-call, so per-context bookkeeping that is saved and restored around a call stays correct
/// when a fiber suspends inside it. An address can be reused by a later fiber once its context
/// is destroyed.
pub fn execution_context() -> usize {
    boundary::execution_context()
}

/// How many times this process has entered the PHP engine. Every entry the
/// bridge makes (running code, calling a callable, firing a PHP callback,
/// releasing a PHP value) is counted; tests use it to show when PHP was not
/// entered at all.
#[doc(hidden)]
pub fn php_entries() -> u64 {
    boundary::entries()
}

/// How many saved per-context records the fiber bookkeeping holds (contexts
/// not running now, abandoned fibers included); tests use it to see records
/// created, freed and forgotten.
#[doc(hidden)]
pub fn fiber_records() -> usize {
    boundary::fiber_records()
}
