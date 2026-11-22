use std::fmt;

/// Why a theme.json operation could not finish.
#[derive(Clone, Debug, PartialEq)]
pub enum ThemeJsonError {
    /// A filter or other outside callback failed.
    Callback(String),
}

impl fmt::Display for ThemeJsonError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ThemeJsonError::Callback(m) => write!(f, "theme.json callback failed: {m}"),
        }
    }
}

impl std::error::Error for ThemeJsonError {}
