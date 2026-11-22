//! Data Mappers between the neutral value and input/output value objects.

use std::any::Any;

use crate::{OperationError, Value};

/// Turns neutral values into an operation's input value object, and its output value object back into neutral values.
/// Rust contracts use `speedyspec_operation::{InputMapper, OutputMapper}`; the PHP facade supplies its own.
pub trait ContractMapper {
    /// The value object's type name, for documents and messages.
    fn type_name(&self) -> &str;
    fn hydrate(&self, value: &Value) -> Result<Box<dyn Any>, OperationError>;
    fn extract(&self, object: &dyn Any) -> Result<Value, OperationError>;
}

/// A Rust input value object built from a (validated) neutral value.
pub trait Hydrate: Sized + 'static {
    fn hydrate(value: &Value) -> Result<Self, OperationError>;
}

/// A Rust output value object turned into a neutral value.
pub trait Extract: 'static {
    fn extract(&self) -> Value;
}
