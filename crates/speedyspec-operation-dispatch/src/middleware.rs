//! Middleware around each operation call.

use std::any::Any;

use speedyspec_operation_contracts::{Caller, OperationError, OperationSpec, Value};

/// What middleware sees of a call: the operation, the caller, the hydrated input and its neutral form.
pub struct Call<'a> {
    pub spec: &'a OperationSpec,
    pub caller: &'a Caller,
    pub input: &'a dyn Any,
    /// The normalized, validated neutral input: what hooks receive.
    pub raw: &'a Value,
}

/// Runs around a call; calls `next` to continue the chain (or does not, to stop it).
pub trait OperationMiddleware {
    fn around(
        &self,
        call: &Call<'_>,
        next: &dyn Fn() -> Result<Box<dyn Any>, OperationError>,
    ) -> Result<Box<dyn Any>, OperationError>;
}

/// Refuses calls the operation's permission check refuses. Always first.
pub struct PermissionMiddleware;

impl OperationMiddleware for PermissionMiddleware {
    fn around(
        &self,
        call: &Call<'_>,
        next: &dyn Fn() -> Result<Box<dyn Any>, OperationError>,
    ) -> Result<Box<dyn Any>, OperationError> {
        call.spec.permission.check(call.caller, call.input)?;
        next()
    }
}

/// A middleware's own answer to a call, in place of the handler's: `Dispatcher::call` returns the value as is, with no
/// output extraction or validation (WordPress does not validate a short-circuit response either).
pub struct Bypass(pub Value);
