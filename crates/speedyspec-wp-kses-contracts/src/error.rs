use std::fmt;

/// Why a kses call could not finish: a hook or a callback failed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum KsesError {
    Hook(String),
    Callback(String),
}

impl fmt::Display for KsesError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            KsesError::Hook(why) => write!(f, "a kses hook failed: {why}"),
            KsesError::Callback(why) => write!(f, "a kses value callback failed: {why}"),
        }
    }
}

impl std::error::Error for KsesError {}
