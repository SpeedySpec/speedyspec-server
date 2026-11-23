//! What the codec reads and writes, and the served method table.

use speedyspec_operation_contracts::{OperationError, Value};

use crate::FaultMapping;

/// A `methodCall`: the method name and its positional parameters.
#[derive(Clone, Debug, PartialEq)]
pub struct MethodCall {
    pub method: String,
    pub params: Vec<Value>,
}

/// A `methodResponse`: one value, or a fault.
#[derive(Clone, Debug, PartialEq)]
pub enum MethodResponse {
    Success(Value),
    Fault(Fault),
}

/// An XML-RPC fault: WordPress's `IXR_Error`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fault {
    pub code: i64,
    pub message: String,
}

impl Fault {
    pub fn new(code: i64, message: impl Into<String>) -> Fault {
        Fault {
            code,
            message: message.into(),
        }
    }

    /// The fault for an operation error: the error's own `details.fault_code` when it has one (an `IXR_Error`'s code
    /// kept through a legacy method), otherwise the mapping's code for its category.
    pub fn from_error(error: &OperationError, mapping: &dyn FaultMapping) -> Fault {
        let code = match error.details.get("fault_code") {
            Some(Value::Int(code)) => *code,
            _ => mapping.code(error.category),
        };
        Fault::new(code, error.message.clone())
    }
}

/// One row of the served method table: a method name and the operation behind it. `legacy` marks an operation an
/// adapter registered for an old-style method; it receives the positional parameters as they came.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MethodEntry {
    pub method: String,
    pub operation: String,
    pub legacy: bool,
}
