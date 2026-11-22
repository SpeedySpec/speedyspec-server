//! Data Mappers for Rust value objects.

use std::any::Any;
use std::marker::PhantomData;

use speedyspec_operation_contracts::{
    Category, ContractMapper, Extract, Hydrate, OperationError, Value,
};

fn wrong_direction(type_name: &str, what: &str) -> OperationError {
    OperationError::new(
        "mapper_direction",
        Category::Internal,
        format!("{type_name} is not an {what} contract"),
    )
}

/// Maps neutral values to the input value object `I`.
pub struct InputMapper<I>(PhantomData<fn() -> I>);

impl<I> InputMapper<I> {
    pub fn new() -> Self {
        InputMapper(PhantomData)
    }
}

impl<I> Default for InputMapper<I> {
    fn default() -> Self {
        InputMapper::new()
    }
}

impl<I: Hydrate> ContractMapper for InputMapper<I> {
    fn type_name(&self) -> &str {
        std::any::type_name::<I>()
    }

    fn hydrate(&self, value: &Value) -> Result<Box<dyn Any>, OperationError> {
        I::hydrate(value).map(|input| Box::new(input) as Box<dyn Any>)
    }

    fn extract(&self, _: &dyn Any) -> Result<Value, OperationError> {
        Err(wrong_direction(self.type_name(), "output"))
    }
}

/// Maps the output value object `O` to neutral values.
pub struct OutputMapper<O>(PhantomData<fn() -> O>);

impl<O> OutputMapper<O> {
    pub fn new() -> Self {
        OutputMapper(PhantomData)
    }
}

impl<O> Default for OutputMapper<O> {
    fn default() -> Self {
        OutputMapper::new()
    }
}

impl<O: Extract> ContractMapper for OutputMapper<O> {
    fn type_name(&self) -> &str {
        std::any::type_name::<O>()
    }

    fn hydrate(&self, _: &Value) -> Result<Box<dyn Any>, OperationError> {
        Err(wrong_direction(self.type_name(), "input"))
    }

    fn extract(&self, object: &dyn Any) -> Result<Value, OperationError> {
        object.downcast_ref::<O>().map(O::extract).ok_or_else(|| {
            OperationError::new(
                "output_type_mismatch",
                Category::Internal,
                format!("the handler did not return a {}", self.type_name()),
            )
        })
    }
}
