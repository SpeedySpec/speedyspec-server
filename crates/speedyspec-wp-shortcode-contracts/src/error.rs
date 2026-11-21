use std::fmt;

/// Why a shortcode operation could not finish.
///
/// A PHP exception thrown by a handler or filter travels as `Handler` or `Filter`, so the bridge can rethrow it
/// unchanged.
#[derive(Clone, Debug, PartialEq)]
pub enum ShortcodeError {
    /// The reference regex hit its backtrack limit or another runtime failure.
    ScanFailed(String),
    /// A handler failed or threw.
    Handler(String),
    /// A filter failed or threw.
    Filter(String),
    /// A value could not be converted to a string as PHP would.
    Conversion(String),
}

impl fmt::Display for ShortcodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ScanFailed(m) => write!(f, "shortcode scan failed: {m}"),
            Self::Handler(m) => write!(f, "shortcode handler failed: {m}"),
            Self::Filter(m) => write!(f, "shortcode filter failed: {m}"),
            Self::Conversion(m) => write!(f, "string conversion failed: {m}"),
        }
    }
}

impl std::error::Error for ShortcodeError {}
