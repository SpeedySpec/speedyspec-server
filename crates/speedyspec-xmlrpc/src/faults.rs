//! The default fault codes (Strategy): WordPress's own.

use speedyspec_operation_contracts::Category;
use speedyspec_xmlrpc_contracts::FaultMapping;

/// The codes WordPress's XML-RPC methods use: 400 for bad arguments, 401 when the user may not, 403 for a failed
/// login, 404 for what does not exist, 500 for the rest. Categories WordPress never uses take their HTTP status.
pub struct DefaultFaultMapping;

impl FaultMapping for DefaultFaultMapping {
    fn code(&self, category: Category) -> i64 {
        match category {
            Category::InvalidArgument => 400,
            Category::PermissionDenied => 401,
            Category::Unauthenticated => 403,
            Category::NotFound => 404,
            Category::AlreadyExists | Category::Conflict => 409,
            Category::ResourceExhausted => 429,
            Category::Unavailable => 503,
            Category::Internal => 500,
        }
    }
}
