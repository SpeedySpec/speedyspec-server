//! Operation handlers and their binding in the container.

use std::any::Any;
use std::rc::Rc;

use speedyspec_container::{Container, ContainerError, Params};
use speedyspec_operation_contracts::{Caller, Category, OperationError, Value};

/// Runs one operation: takes its input value object, returns its output value object.
pub trait OperationHandler {
    fn handle(&self, input: Box<dyn Any>, caller: &Caller) -> Result<Box<dyn Any>, OperationError>;

    /// Whether this handler takes and returns neutral values ([`OperationHandler::handle_neutral`]) instead of value
    /// objects. A handler that answers for contracts it does not know, such as a PHP handler replacing a Rust one, is
    /// neutral. Its input is still validated, its permission still checked and its output still validated.
    fn neutral(&self) -> bool {
        false
    }

    /// Runs the operation over the (normalized, validated) neutral input. Called only when [`neutral`](Self::neutral)
    /// is true.
    fn handle_neutral(&self, _input: &Value, _caller: &Caller) -> Result<Value, OperationError> {
        Err(OperationError::new(
            "handler_not_neutral",
            Category::Internal,
            "this handler takes value objects, not neutral values",
        ))
    }
}

/// The container key of an operation's handler.
pub fn handler_key(name: &str) -> String {
    format!("operation:{name}")
}

/// Binds (or rebinds) the handler of the operation `name`, shared for the container's life.
pub fn bind_handler(
    container: &Container,
    name: &str,
    factory: impl Fn(&Container, &Params) -> Result<Rc<dyn OperationHandler>, ContainerError> + 'static,
) {
    container.singleton_port::<dyn OperationHandler>(&handler_key(name), factory);
}

/// A handler from a closure over typed value objects.
pub fn handler_fn<I: 'static, O: 'static>(
    f: impl Fn(I, &Caller) -> Result<O, OperationError> + 'static,
) -> Rc<dyn OperationHandler> {
    Rc::new(FnHandler(Box::new(f)))
}

type Handle<I, O> = Box<dyn Fn(I, &Caller) -> Result<O, OperationError>>;

struct FnHandler<I, O>(Handle<I, O>);

impl<I: 'static, O: 'static> OperationHandler for FnHandler<I, O> {
    fn handle(&self, input: Box<dyn Any>, caller: &Caller) -> Result<Box<dyn Any>, OperationError> {
        let input = input.downcast::<I>().map_err(|_| {
            OperationError::new(
                "input_type_mismatch",
                Category::Internal,
                format!("the handler expected a {}", std::any::type_name::<I>()),
            )
        })?;
        (self.0)(*input, caller).map(|output| Box::new(output) as Box<dyn Any>)
    }
}
