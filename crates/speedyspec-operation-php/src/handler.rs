//! PHP callables as operation handlers.

use std::any::Any;

use speedyspec_operation_contracts::{Caller, Category, OperationError};
use speedyspec_operation_dispatch::OperationHandler;
use speedyspec_php_ext::Value as PhpValue;

use crate::area::php_caller;
use crate::invoke;

/// A PHP handler, `fn( Input $in, Caller $caller ): Output|array`, bound for one request with `Handler::for`.
pub struct PhpOperationHandler {
    pub operation: String,
    pub callable: PhpValue,
}

impl OperationHandler for PhpOperationHandler {
    fn handle(&self, input: Box<dyn Any>, caller: &Caller) -> Result<Box<dyn Any>, OperationError> {
        // A PHP handler takes the PHP value object the operation's PHP mapper built. An operation with Rust contracts
        // hands its handler a Rust struct, which PHP cannot take.
        let input = input.downcast::<PhpValue>().map_err(|_| {
            OperationError::new(
                "handler_contract_mismatch",
                Category::Internal,
                format!(
                    "a PHP handler cannot take {}'s Rust input value object",
                    self.operation
                ),
            )
        })?;
        let caller = php_caller(caller)?;
        invoke::run(&self.callable, vec![*input, caller])
            .map(|output| Box::new(output) as Box<dyn Any>)
    }
}
