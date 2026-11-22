//! Calls into PHP callbacks that answer for an operation (handlers, permission callbacks), with their failures as
//! operation errors.
//!
//! `Internal\Invoker::run` calls the callback and catches what it throws: an `OperationException` keeps its code,
//! category, message and details; any other `Throwable` is logged with `error_log()` and becomes a generic
//! `handler_failed`, so a client never sees a path, a query or a stack trace.

use speedyspec_operation_contracts::{Category, OperationError, Value};
use speedyspec_php_ext::{Key, Value as PhpValue};

use crate::convert::from_php;

const INVOKER: &str = "SpeedySpec\\Operation\\Internal\\Invoker::run";

fn field<'a>(items: &'a [(Key, PhpValue)], name: &str) -> Option<&'a PhpValue> {
    items
        .iter()
        .find(|(k, _)| *k == Key::Str(name.to_owned()))
        .map(|(_, v)| v)
}

fn failed(why: impl Into<String>) -> OperationError {
    OperationError::new("handler_failed", Category::Internal, why)
}

/// Runs `callable(...args)` in PHP.
pub fn run(callable: &PhpValue, args: Vec<PhpValue>) -> Result<PhpValue, OperationError> {
    let list = PhpValue::Array(
        args.into_iter()
            .enumerate()
            .map(|(i, v)| (Key::Int(i as i64), v))
            .collect(),
    );
    let outcome = speedyspec_php::call(&PhpValue::Str(INVOKER.into()), &[callable.clone(), list])
        .map_err(|err| {
        eprintln!("speedyspec_wordpress: calling an operation callback failed: {err}");
        failed("The operation failed.")
    })?;
    let PhpValue::Array(items) = &outcome else {
        return Err(failed("The operation failed."));
    };
    if let Some(result) = field(items, "ok") {
        return Ok(result.clone());
    }
    let Some(PhpValue::Array(error)) = field(items, "error") else {
        return Err(failed("The operation failed."));
    };
    let text = |name: &str| match field(error, name) {
        Some(PhpValue::Str(s)) => s.clone(),
        _ => String::new(),
    };
    let category = Category::parse(&text("category")).unwrap_or(Category::Internal);
    let details = field(error, "details")
        .map(from_php)
        .transpose()?
        .unwrap_or(Value::Null);
    let details = match details {
        Value::List(l) if l.is_empty() => Value::Null,
        other => other,
    };
    Err(OperationError::new(text("code"), category, text("message")).with_details(details))
}
