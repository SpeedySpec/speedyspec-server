//! The PHP errors the write use cases raise where WordPress's own code would throw (a `TypeError` from `array_merge()`
//! on a scalar, an `Error` from using a scalar as an array).
//!
//! They travel as [`PostError::Hook`] with a [`PhpThrow`] payload, so the use case stops there as WordPress does, and
//! the PHP binding throws an instance of [`PhpThrow::class`] with [`PhpThrow::message`] where WordPress would.

use std::fmt;
use std::rc::Rc;

use speedyspec_wp_hooks_contracts::HookError;
use speedyspec_wp_posts_contracts::PostError;

/// A PHP error WordPress's code would raise: its class (`TypeError`, `Error`) and message, as PHP 8.5 words them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PhpThrow {
    /// The error's class.
    pub class: &'static str,
    /// The error's message.
    pub message: String,
}

impl fmt::Display for PhpThrow {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.class, self.message)
    }
}

/// The [`PostError`] that carries a [`PhpThrow`].
pub(crate) fn php_error(class: &'static str, message: String) -> PostError {
    let thrown = PhpThrow { class, message };
    PostError::Hook(HookError {
        message: thrown.to_string(),
        payload: Some(Rc::new(thrown)),
    })
}

/// The [`PhpThrow`] a failure carries, if it is one.
pub fn php_throw(error: &PostError) -> Option<&PhpThrow> {
    match error {
        PostError::Hook(hook) => hook.payload.as_ref()?.downcast_ref::<PhpThrow>(),
        _ => None,
    }
}
