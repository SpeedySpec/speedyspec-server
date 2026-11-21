//! A failed port call.

use std::any::Any;
use std::fmt;
use std::rc::Rc;

/// A port call that failed: a PHP adapter's callback threw, or an answer could not be read.
///
/// It has the shape of the Hook API's `HookError`, so a PHP exception passes through formatting unchanged; the
/// conversion between the two lives in the crates that depend on both.
#[derive(Clone)]
pub struct TextError {
    pub message: String,
    /// What a language bridge needs to rethrow the original failure (a PHP exception object).
    pub payload: Option<Rc<dyn Any>>,
}

impl TextError {
    pub fn message(message: impl Into<String>) -> TextError {
        TextError {
            message: message.into(),
            payload: None,
        }
    }
}

impl fmt::Debug for TextError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "TextError({:?})", self.message)
    }
}

impl fmt::Display for TextError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for TextError {}
