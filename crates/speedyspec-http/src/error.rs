use std::fmt;

/// Why an HTTP value was refused or could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HttpError {
    InvalidHeaderName(String),
    InvalidHeaderValue(String),
    InvalidStatus(u16),
    InvalidUri(String),
    Io(String),
    /// The body was already written to the client (by PHP), so it cannot be read or replaced.
    BodyAlreadySent,
}

impl fmt::Display for HttpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HttpError::InvalidHeaderName(name) => write!(f, "invalid header name: {name:?}"),
            HttpError::InvalidHeaderValue(value) => write!(f, "invalid header value: {value:?}"),
            HttpError::InvalidStatus(code) => write!(f, "invalid status code: {code}"),
            HttpError::InvalidUri(uri) => write!(f, "invalid URI: {uri}"),
            HttpError::Io(err) => write!(f, "i/o error: {err}"),
            HttpError::BodyAlreadySent => write!(f, "the body was already sent"),
        }
    }
}

impl std::error::Error for HttpError {}
