//! Calling operations by name.

use std::any::Any;
use std::rc::Rc;

use speedyspec_container::Container;
use speedyspec_operation::Registries;
use speedyspec_operation::validate::{normalize, validate, violations_value};
use speedyspec_operation_contracts::{
    Caller, Category, INTERNAL_TRANSPORT, Kind, OperationCatalog, OperationError, OperationSpec,
    Value,
};

use crate::{
    Bypass, Call, OperationHandler, OperationMiddleware, PermissionMiddleware, handler_key,
};

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

    /// The operation `name` as `caller` may reach it: registration closed, exposed on the caller's transport, not a
    /// subscription.
    fn spec(&self, name: &str, transport: &str) -> Result<Rc<OperationSpec>, OperationError> {
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
            .filter(|spec| spec.exposed_on(transport))
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
        Ok(spec)
    }

    /// The normalized input, validated against the operation's input schema.
    fn checked_input(&self, spec: &OperationSpec, input: &Value) -> Result<Value, OperationError> {
        let input = normalize(&spec.input.schema, input, self.registries.as_ref());
        validate(&spec.input.schema, &input, self.registries.as_ref()).map_err(|violations| {
            OperationError::new(
                "invalid_input",
                Category::InvalidArgument,
                format!("invalid input for {}", spec.name.as_str()),
            )
            .with_details(violations_value(&violations))
        })?;
        Ok(input)
    }

    /// Validates `input` against the operation's input schema, as [`Dispatcher::call`] does before the handler, for
    /// transports that check input as a step of their own (the Abilities API). Reachability is checked from the
    /// internal transport.
    pub fn validate(&self, name: &str, input: &Value) -> Result<(), OperationError> {
        let spec = self.spec(name, INTERNAL_TRANSPORT)?;
        self.checked_input(&spec, input).map(drop)
    }

    /// Runs the operation's permission check for `caller` on `input` (normalized, validated and hydrated first), as
    /// [`Dispatcher::call`] does before the handler.
    pub fn authorize(
        &self,
        name: &str,
        input: &Value,
        caller: &Caller,
    ) -> Result<(), OperationError> {
        let spec = self.spec(name, &caller.transport)?;
        let input = self.checked_input(&spec, input)?;
        let hydrated = spec.input.mapper.hydrate(&input)?;
        spec.permission.check(caller, hydrated.as_ref())
    }

    pub fn call(&self, name: &str, input: Value, caller: &Caller) -> Result<Value, OperationError> {
        self.dispatch(name, input, caller, &self.middleware)
    }

    /// Calls the operation with every middleware but the permission check, for a transport that decided permission
    /// itself after [`Dispatcher::authorize`] (WordPress's `wp_ability_permission_result` can grant what the check
    /// refused). Input and output are validated as in [`Dispatcher::call`].
    pub fn call_authorized(
        &self,
        name: &str,
        input: Value,
        caller: &Caller,
    ) -> Result<Value, OperationError> {
        // The permission middleware is always the first.
        self.dispatch(name, input, caller, &self.middleware[1..])
    }

    fn dispatch(
        &self,
        name: &str,
        input: Value,
        caller: &Caller,
        middleware: &[Rc<dyn OperationMiddleware>],
    ) -> Result<Value, OperationError> {
        let spec = self.spec(name, &caller.transport)?;
        let input = self.checked_input(&spec, &input)?;
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
            raw: &input,
        };
        let run_handler = || {
            if handler.neutral() {
                handler
                    .handle_neutral(&input, caller)
                    .map(|output| Box::new(NeutralOutput(output)) as Box<dyn Any>)
            } else {
                handler.handle(spec.input.mapper.hydrate(&input)?, caller)
            }
        };
        let output = match run(middleware, &call, &run_handler)?.downcast::<Bypass>() {
            Ok(bypass) => return Ok(bypass.0),
            Err(output) => output,
        };
        let plain = match output.downcast::<NeutralOutput>() {
            Ok(neutral) => neutral.0,
            Err(output) => spec.output.mapper.extract(output.as_ref())?,
        };
        let extracted = normalize(&spec.output.schema, &plain, self.registries.as_ref());
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

/// A neutral handler's output: a value already, so it skips extraction (but not validation).
struct NeutralOutput(Value);

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
