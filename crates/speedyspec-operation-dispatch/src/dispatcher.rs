//! Calling operations by name.

use std::any::Any;
use std::rc::Rc;

use speedyspec_container::Container;
use speedyspec_operation::Registries;
use speedyspec_operation::validate::{validate, violations_value};
use speedyspec_operation_contracts::{
    Caller, Category, Kind, OperationCatalog, OperationError, Value,
};

use crate::{Call, OperationHandler, OperationMiddleware, PermissionMiddleware, handler_key};

/// Calls operations by name with neutral values.
pub struct Dispatcher {
    container: Rc<Container>,
    registries: Rc<Registries>,
    middleware: Vec<Rc<dyn OperationMiddleware>>,
}

impl Dispatcher {
    pub fn new(container: Rc<Container>, registries: Rc<Registries>) -> Self {
        Dispatcher {
            container,
            registries,
            middleware: vec![Rc::new(PermissionMiddleware)],
        }
    }

    /// Adds middleware after the permission check; middleware runs in the order added.
    pub fn with_middleware(mut self, middleware: Rc<dyn OperationMiddleware>) -> Self {
        self.middleware.push(middleware);
        self
    }

    pub fn call(&self, name: &str, input: Value, caller: &Caller) -> Result<Value, OperationError> {
        if !self.registries.is_closed() {
            return Err(OperationError::new(
                "registry_open",
                Category::Internal,
                "operations cannot be called before registration closes",
            ));
        }
        let spec = self
            .registries
            .get_operation(name)
            .filter(|spec| spec.exposed_on(&caller.transport))
            .ok_or_else(|| {
                OperationError::new(
                    "operation_not_found",
                    Category::NotFound,
                    format!("no operation {name}"),
                )
            })?;
        if spec.kind == Kind::Subscription {
            return Err(OperationError::new(
                "subscriptions_not_served",
                Category::Unavailable,
                format!("{name} is a subscription, which is not served yet"),
            ));
        }
        validate(&spec.input.schema, &input, self.registries.as_ref()).map_err(|violations| {
            OperationError::new(
                "invalid_input",
                Category::InvalidArgument,
                format!("invalid input for {name}"),
            )
            .with_details(violations_value(&violations))
        })?;
        let handler = self
            .container
            .make_port::<dyn OperationHandler>(&handler_key(name))
            .map_err(|e| {
                OperationError::new("operation_no_handler", Category::Internal, e.to_string())
            })?;

        // Middleware reads the input by reference while the handler takes it by value, inside the same chain, so the
        // handler gets its own hydration of the (already validated) input. Mappers are pure, so both are equal.
        let seen = spec.input.mapper.hydrate(&input)?;
        let call = Call {
            spec: &spec,
            caller,
            input: seen.as_ref(),
        };
        let run_handler = || handler.handle(spec.input.mapper.hydrate(&input)?, caller);
        let output = run(&self.middleware, &call, &run_handler)?;
        let extracted = spec.output.mapper.extract(output.as_ref())?;
        validate(&spec.output.schema, &extracted, self.registries.as_ref()).map_err(
            |violations| {
                OperationError::new(
                    "invalid_output",
                    Category::Internal,
                    format!("{name} returned an invalid output"),
                )
                .with_details(violations_value(&violations))
            },
        )?;
        Ok(extracted)
    }
}

/// Runs `middleware` in order, then `handle`.
fn run(
    middleware: &[Rc<dyn OperationMiddleware>],
    call: &Call<'_>,
    handle: &dyn Fn() -> Result<Box<dyn Any>, OperationError>,
) -> Result<Box<dyn Any>, OperationError> {
    match middleware.split_first() {
        None => handle(),
        Some((first, rest)) => first.around(call, &|| run(rest, call, handle)),
    }
}
