//! PHP callables as operation handlers.

use std::any::Any;

use speedyspec_operation_contracts::{Caller, Category, OperationError, Value};
use speedyspec_operation_dispatch::OperationHandler;
use speedyspec_php_ext::Value as PhpValue;

use crate::area::php_caller;
use crate::convert::{from_php, to_php};
use crate::invoke;

/// A PHP handler, `fn( Input $in, Caller $caller ): Output|array`, bound for one request with `Handler::for`. For an
/// operation with Rust contracts it is neutral: `$in` is a `SpeedySpec\Operation\Proxy` over the input.
pub struct PhpOperationHandler {
    pub operation: String,
    pub callable: PhpValue,
}

impl OperationHandler for PhpOperationHandler {
    /// Neutral for an operation PHP did not register: its contracts are Rust structs, which PHP cannot take, so the
    /// handler takes the validated input as a value instead (as a read-only `SpeedySpec\Operation\Proxy`).
    fn neutral(&self) -> bool {
        crate::area::state()
            .map(|state| !state.registering().is_php_operation(&self.operation))
            .unwrap_or(false)
    }

    fn handle_neutral(&self, input: &Value, caller: &Caller) -> Result<Value, OperationError> {
        let proxy = speedyspec_php::call(
            &PhpValue::Str("SpeedySpec\\Operation\\Internal\\Callers::proxy".into()),
            &[to_php(input)],
        )
        .map_err(|err| OperationError::new("proxy_failed", Category::Internal, err.to_string()))?;
        let caller = php_caller(caller)?;
        let output = invoke::run(&self.callable, vec![proxy, caller])?;
        let plain = speedyspec_php::call(
            &PhpValue::Str("SpeedySpec\\Operation\\Internal\\Mapper::extract".into()),
            &[output],
        )
        .map_err(|err| {
            OperationError::new("extraction_failed", Category::Internal, err.to_string())
        })?;
        from_php(&plain)
    }

    fn handle(&self, input: Box<dyn Any>, caller: &Caller) -> Result<Box<dyn Any>, OperationError> {
        // A PHP handler takes the PHP value object the operation's PHP mapper built.
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
