//! Which operations each transport exposes.

use std::rc::Rc;

use speedyspec_operation_contracts::{OperationSpec, Value};

/// Where an exposed operation's binding came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BindingSource {
    Explicit,
    Default,
}

/// An operation one transport exposes, with the binding it uses.
pub struct ExposedOperation {
    pub spec: Rc<OperationSpec>,
    pub binding: Value,
    pub source: BindingSource,
}

/// Reads what each transport exposes.
pub trait ExposureLookup {
    fn find(&self, transport: &str, operation: &str) -> Option<Rc<ExposedOperation>>;
    /// Ordered by operation name.
    fn exposed(&self, transport: &str) -> Vec<Rc<ExposedOperation>>;
}

/// Why exposure could not be computed, or a transport not installed.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum ExposureError {
    #[error("operations cannot be exposed before registration closes")]
    RegistryOpen,
    #[error("the transport {transport} is already installed")]
    DuplicateTransport { transport: String },
    #[error("{operation} is bound on {transport} but is not registered")]
    UnknownOperation {
        operation: String,
        transport: String,
    },
    #[error("{operation} is bound on {transport} but restricted to other transports")]
    OutsideRestriction {
        operation: String,
        transport: String,
    },
    #[error("{operation} has an invalid binding on {transport}: {why}")]
    InvalidBinding {
        operation: String,
        transport: String,
        why: String,
    },
}
