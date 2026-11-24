//! Abilities that are not operations (`wp_register_ability`, subclasses, abilities never registered) as operations:
//! the LegacyHandlerAdapter.
//!
//! Each `WP_Ability` object executed in a request is an operation `legacy-abilities/{slug}-{hash}`, restricted to
//! `abilities`, with an open input and a public permission (the ability's own `check_permissions` is a step the
//! Abilities server runs). Its handler runs the ability's `do_execute`. The table is per request, keyed by the
//! object, and created when an ability first executes, so abilities built directly (as WordPress's tests build them)
//! execute too.

use std::any::Any;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use speedyspec_container::{Container, Lifetime};
use speedyspec_operation::{Operation, Registries, S};
use speedyspec_operation_contracts::{
    Caller, Category, OperationError, OperationSpec, RawInput, RawOutput, Value,
};
use speedyspec_operation_dispatch::{Dispatcher, OperationHandler, bind_handler};

use crate::steps::step;

/// One legacy ability of the request.
struct Entry {
    spec: OperationSpec,
    dispatcher: Rc<Dispatcher>,
    /// The handle of the object for the call in progress.
    handle: Rc<std::cell::Cell<u64>>,
}

thread_local! {
    static ENTRIES: RefCell<HashMap<i64, Entry>> = RefCell::new(HashMap::new());
}

/// Forgets the request's legacy abilities.
pub fn clear() {
    ENTRIES.with(|e| e.borrow_mut().clear());
}

/// The request's legacy abilities' operations.
pub fn operations() -> Vec<OperationSpec> {
    ENTRIES.with(|e| {
        e.borrow()
            .values()
            .map(|entry| entry.spec.clone())
            .collect()
    })
}

fn fnv(text: &str) -> u32 {
    text.bytes().fold(0x811c_9dc5_u32, |h, b| {
        (h ^ u32::from(b)).wrapping_mul(0x0100_0193)
    })
}

/// The operation name of the legacy ability `name` held by the object `object`.
pub fn operation_name(name: &str, object: i64) -> String {
    let mut slug = String::new();
    for c in name.to_ascii_lowercase().chars() {
        if c.is_ascii_lowercase() || c.is_ascii_digit() {
            slug.push(c);
        } else if !slug.ends_with('-') {
            slug.push('-');
        }
    }
    let slug = slug.trim_matches('-');
    let slug = if slug.is_empty() { "ability" } else { slug };
    format!(
        "legacy-abilities/{slug}-{:08x}",
        fnv(&format!("{name}\u{0}{object}"))
    )
}

/// The dispatcher and operation of the object `object` (the ability `name`), registered on its first call, with the
/// call's `handle` recorded.
pub(crate) fn operation_for(
    container: &Rc<Container>,
    name: &str,
    object: i64,
    handle: u64,
) -> Result<(Rc<Dispatcher>, String), OperationError> {
    if let Some(found) = ENTRIES.with(|e| {
        e.borrow().get(&object).map(|entry| {
            entry.handle.set(handle);
            (
                entry.dispatcher.clone(),
                entry.spec.name.as_str().to_owned(),
            )
        })
    }) {
        return Ok(found);
    }
    let operation = operation_name(name, object);
    let internal = |e: speedyspec_operation::RegistrationError| {
        OperationError::new("ability_unavailable", Category::Internal, e.to_string())
    };
    let spec = Operation::command(&operation)
        .input::<RawInput>(S::any())
        .output::<RawOutput>(S::any())
        .transports(&[speedyspec_abilities::ABILITIES])
        .public()
        .build()
        .map_err(internal)?;
    let registries = Registries::new();
    registries
        .register_operation(spec.clone())
        .map_err(internal)?;
    registries.close().map_err(|errors| {
        OperationError::new(
            "ability_unavailable",
            Category::Internal,
            errors
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("; "),
        )
    })?;
    let handle_cell = Rc::new(std::cell::Cell::new(handle));
    let handler: Rc<dyn OperationHandler> = Rc::new(LegacyHandler {
        handle: handle_cell.clone(),
    });
    {
        let _request = container.registering(Lifetime::Request);
        bind_handler(container, &operation, move |_, _| Ok(handler.clone()));
    }
    let dispatcher = Rc::new(Dispatcher::new(container.clone(), Rc::new(registries)));
    ENTRIES.with(|e| {
        e.borrow_mut().insert(
            object,
            Entry {
                spec,
                dispatcher: dispatcher.clone(),
                handle: handle_cell,
            },
        )
    });
    Ok((dispatcher, operation))
}

/// Runs a legacy ability's `do_execute` (its `wp_ability_execute_result` filter included).
struct LegacyHandler {
    handle: Rc<std::cell::Cell<u64>>,
}

impl OperationHandler for LegacyHandler {
    fn handle(&self, _: Box<dyn Any>, _: &Caller) -> Result<Box<dyn Any>, OperationError> {
        Err(OperationError::new(
            "handler_not_neutral",
            Category::Internal,
            "a legacy ability handler is neutral",
        ))
    }

    fn neutral(&self) -> bool {
        true
    }

    fn handle_neutral(&self, input: &Value, _: &Caller) -> Result<Value, OperationError> {
        let input = match input.get("input") {
            None | Some(Value::Null) => None,
            Some(value) => Some(value.clone()),
        };
        step(self.handle.get(), "do_execute", &input).map(|v| crate::values::neutral(&v))
    }
}

/// The input a legacy ability's operation is called with.
pub(crate) fn input_value(input: &Option<Value>) -> Value {
    Value::map([("input", input.clone().unwrap_or(Value::Null))])
}
