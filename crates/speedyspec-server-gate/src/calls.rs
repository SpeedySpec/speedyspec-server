//! The port the gate, the health checks and maintenance use to reach a PHP app: its implementation sends internal
//! requests over FastCGI, on the app's internal worker.

use std::future::Future;
use std::pin::Pin;

use speedyspec_server_domain::AppId;

/// A boxed future, so [`GateCalls`] stays object-safe.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Why an internal call has no usable answer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CallError {
    /// No answer within the call's timeout.
    Timeout,
    /// The app could not answer (not running, crashed, not a script).
    Unavailable(String),
    /// It answered, but not with what the contract says.
    Malformed(String),
}

impl std::fmt::Display for CallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CallError::Timeout => f.write_str("no answer in time"),
            CallError::Unavailable(why) => write!(f, "unavailable: {why}"),
            CallError::Malformed(why) => write!(f, "malformed answer: {why}"),
        }
    }
}

/// The server's own calls to PHP apps.
pub trait GateCalls: Send + Sync {
    /// `GET <path>` on `app`: the answer's status.
    fn probe<'a>(&'a self, app: &'a AppId, path: &'a str) -> BoxFuture<'a, Result<u16, CallError>>;
}
