//! Who is calling: the identity a transport authenticated, and the capability check behind it.

use std::any::Any;
use std::rc::Rc;

use crate::{OperationError, Value};

/// The transport name of calls made from inside the application; every operation is reachable from it.
pub const INTERNAL_TRANSPORT: &str = "internal";

/// Driven port for capability checks. A PHP adapter (`current_user_can`) comes first; a Rust one replaces it once users
/// and capabilities are ported.
pub trait Identity {
    fn can(&self, user: Option<u64>, capability: &str, args: &[Value]) -> bool;
}

/// The caller of one operation: the user a transport authenticated (none when anonymous), which transport, and the
/// fields the client asked for (GraphQL selections, REST `_fields`).
#[derive(Clone)]
pub struct Caller {
    pub user: Option<u64>,
    pub transport: String,
    pub identity: Rc<dyn Identity>,
    pub requested_fields: Option<Vec<String>>,
}

impl Caller {
    pub fn new(user: Option<u64>, transport: &str, identity: Rc<dyn Identity>) -> Self {
        Caller {
            user,
            transport: transport.to_owned(),
            identity,
            requested_fields: None,
        }
    }

    pub fn with_requested_fields(mut self, fields: Vec<String>) -> Self {
        self.requested_fields = Some(fields);
        self
    }

    pub fn can(&self, capability: &str, args: &[Value]) -> bool {
        self.identity.can(self.user, capability, args)
    }
}

/// Whether a caller may run an operation with a given (hydrated) input. Part of the operation's contract.
pub trait PermissionCheck {
    fn check(&self, caller: &Caller, input: &dyn Any) -> Result<(), OperationError>;
}
