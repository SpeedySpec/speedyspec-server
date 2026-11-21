use std::any::Any;
use std::fmt;
use std::rc::Rc;

/// Why a shortcode operation could not finish.
///
/// A foreign exception (a PHP `Throwable`) thrown by a handler or filter travels as [`ShortcodeError::Thrown`]
/// with its payload, so the PHP bridge rethrows it unchanged.
#[derive(Clone)]
pub enum ShortcodeError {
    /// The reference regex hit its backtrack limit or another runtime failure.
    ScanFailed(String),
    /// A handler failed.
    Handler(String),
    /// A filter failed.
    Filter(String),
    /// A value could not be converted to a string as PHP would.
    Conversion(String),
    /// A handler or filter threw a foreign exception.
    Thrown(Thrown),
}

/// A foreign exception, carried without looking inside.
#[derive(Clone)]
pub struct Thrown {
    pub message: String,
    pub payload: Rc<dyn Any>,
}

impl ShortcodeError {
    fn message(&self) -> &str {
        match self {
            Self::ScanFailed(m) | Self::Handler(m) | Self::Filter(m) | Self::Conversion(m) => m,
            Self::Thrown(t) => &t.message,
        }
    }

    fn kind(&self) -> &'static str {
        match self {
            Self::ScanFailed(_) => "shortcode scan failed",
            Self::Handler(_) => "shortcode handler failed",
            Self::Filter(_) => "shortcode filter failed",
            Self::Conversion(_) => "string conversion failed",
            Self::Thrown(_) => "thrown",
        }
    }
}

impl PartialEq for ShortcodeError {
    /// Compares kind and message; a thrown payload is compared by identity.
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Thrown(a), Self::Thrown(b)) => a.message == b.message && Rc::ptr_eq(&a.payload, &b.payload),
            _ => self.kind() == other.kind() && self.message() == other.message(),
        }
    }
}

impl fmt::Debug for ShortcodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ShortcodeError({}: {:?})", self.kind(), self.message())
    }
}

impl fmt::Display for ShortcodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.kind(), self.message())
    }
}

impl std::error::Error for ShortcodeError {}
