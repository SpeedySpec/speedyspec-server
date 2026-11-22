//! The transport ports.

use speedyspec_operation_contracts::{Caller, OperationError, OperationSpec, Value};

use crate::{ExposureLookup, TransportName};

/// A protocol that exposes operations.
pub trait Transport {
    fn name(&self) -> &TransportName;
    /// The binding (route, method name, …) for an operation with no explicit one: this transport's default mapping.
    /// `None`: this transport cannot expose the operation by default.
    fn default_binding(&self, spec: &OperationSpec) -> Option<Value>;
    /// Whether an explicit binding is one this transport can use; `Err` says why not.
    fn check_binding(&self, spec: &OperationSpec, binding: &Value) -> Result<(), String>;
}

/// Per-operation, per-transport bindings, opaque to everything but the named transport.
pub trait BindingStore {
    /// Sets (or replaces) the binding of `operation` on `transport`.
    fn bind(&self, operation: &str, transport: &str, config: Value);
    fn binding(&self, operation: &str, transport: &str) -> Option<Value>;
    /// Every binding as `(operation, transport, config)`, ordered by operation, then transport.
    fn bindings(&self) -> Vec<(String, String, Value)>;
}

/// What a codec read from a request: which operation, with what input.
#[derive(Clone, Debug, PartialEq)]
pub struct DecodedCall {
    pub operation: String,
    pub input: Value,
    pub requested_fields: Option<Vec<String>>,
}

/// Turns a transport's credentials into a caller.
pub trait Authenticator<R> {
    fn authenticate(&self, request: &R) -> Result<Caller, OperationError>;
}

/// Turns a transport's requests into calls, and outputs and errors into its responses.
pub trait Codec<R, S> {
    fn decode(
        &self,
        request: &R,
        exposure: &dyn ExposureLookup,
    ) -> Result<DecodedCall, OperationError>;
    fn encode(&self, request: &R, call: &DecodedCall, output: Value) -> S;
    fn encode_error(&self, request: &R, error: &OperationError) -> S;
}

/// WordPress-style hooks, over neutral values.
pub trait TransportHooks {
    fn action(&self, hook: &str, args: Vec<Value>) -> Result<(), OperationError>;
    fn filter(&self, hook: &str, value: Value, args: Vec<Value>) -> Result<Value, OperationError>;
}

/// A transport's hooks at the point between the permission check and the handler.
pub trait StageHooks {
    /// `Some(value)` short-circuits the call with `value` as its output.
    fn before_handler(
        &self,
        spec: &OperationSpec,
        caller: &Caller,
        input: &Value,
        hooks: &dyn TransportHooks,
    ) -> Result<Option<Value>, OperationError>;
}
