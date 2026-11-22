//! Operation errors and their categories.

use crate::Value;

/// What kind of failure an error is. Transports map categories to their own codes (HTTP status, XML-RPC fault code,
/// GraphQL `extensions.code`). The set is gRPC's canonical codes, the best-tested list for mapping onto every protocol.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Category {
    NotFound,
    PermissionDenied,
    Unauthenticated,
    InvalidArgument,
    AlreadyExists,
    Conflict,
    ResourceExhausted,
    Unavailable,
    Internal,
}

impl Category {
    /// The category's canonical snake_case name.
    pub fn as_str(&self) -> &'static str {
        match self {
            Category::NotFound => "not_found",
            Category::PermissionDenied => "permission_denied",
            Category::Unauthenticated => "unauthenticated",
            Category::InvalidArgument => "invalid_argument",
            Category::AlreadyExists => "already_exists",
            Category::Conflict => "conflict",
            Category::ResourceExhausted => "resource_exhausted",
            Category::Unavailable => "unavailable",
            Category::Internal => "internal",
        }
    }
}

/// Why an operation failed: a stable code, its category, a human message and structured details.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
#[error("{code}: {message}")]
pub struct OperationError {
    pub code: String,
    pub category: Category,
    pub message: String,
    /// Boxed so a `Result<_, OperationError>` stays small.
    pub details: Box<Value>,
}

impl OperationError {
    pub fn new(code: impl Into<String>, category: Category, message: impl Into<String>) -> Self {
        OperationError {
            code: code.into(),
            category,
            message: message.into(),
            details: Box::new(Value::Null),
        }
    }

    pub fn with_details(mut self, details: Value) -> Self {
        self.details = Box::new(details);
        self
    }
}
