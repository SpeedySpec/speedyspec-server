use std::fmt;

/// Sends a request and returns its response (PSR-18's `ClientInterface`).
///
/// Named here for the later port of WordPress's outgoing HTTP client; nothing implements it yet.
pub trait Client<Req, Res> {
    fn send(&self, request: Req) -> Result<Res, ClientError>;
}

/// Why a request could not be sent or answered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientError(pub String);

impl fmt::Display for ClientError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "http client error: {}", self.0)
    }
}

impl std::error::Error for ClientError {}
