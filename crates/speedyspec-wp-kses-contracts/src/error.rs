use std::any::Any;
use std::fmt;
use std::rc::Rc;

/// What a language bridge needs to rethrow a failure unchanged (a PHP exception object). Equal only to itself.
#[derive(Clone)]
pub struct Payload(pub Rc<dyn Any>);

impl fmt::Debug for Payload {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Payload(..)")
    }
}

impl PartialEq for Payload {
    fn eq(&self, other: &Payload) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for Payload {}

/// Why a kses call could not finish: a hook or a callback failed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum KsesError {
    Hook(String),
    Callback(String),
    /// A hook or callback in another language failed; `payload` is that failure, for its bridge to rethrow.
    Foreign {
        message: String,
        payload: Payload,
    },
}

impl fmt::Display for KsesError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            KsesError::Hook(why) => write!(f, "a kses hook failed: {why}"),
            KsesError::Callback(why) => write!(f, "a kses value callback failed: {why}"),
            KsesError::Foreign { message, .. } => {
                write!(f, "a kses hook or callback failed: {message}")
            }
        }
    }
}

impl std::error::Error for KsesError {}
