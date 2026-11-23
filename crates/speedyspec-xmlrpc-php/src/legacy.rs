//! Legacy XML-RPC methods (`xmlrpc_methods`) as operations: the LegacyHandlerAdapter.
//!
//! The method table belongs to the request's `wp_xmlrpc_server`, as in WordPress: every entry that is not an
//! operation (WordPress's own `this:` methods, a plugin's functions and static methods) becomes an operation
//! restricted to `xmlrpc`, named `legacy-xmlrpc/{slug}-{hash}`, with an open input. Its handler runs the entry as
//! `IXR_Server::call()` does. The table is request-scoped and kept apart from the site's closed registries;
//! `XmlRpcService` serves the site's operations plus this table. A new `wp_xmlrpc_server` starts over.

use std::any::Any;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use indexmap::IndexMap;
use speedyspec_container::{Container, Lifetime};
use speedyspec_operation::{Operation, S};
use speedyspec_operation_contracts::{
    Caller, Category, OperationError, OperationSpec, RawInput, RawOutput, Value,
};
use speedyspec_operation_dispatch::{OperationHandler, bind_handler};
use speedyspec_php_ext::{Key, Value as PhpValue};
use speedyspec_xmlrpc::{XMLRPC, error_of};
use speedyspec_xmlrpc_contracts::{Fault, MethodEntry};

use crate::convert::{adapter, from_tagged, to_tagged};

thread_local! {
    /// This request's table entries (the PHP callbacks), by legacy operation.
    static CALLBACKS: RefCell<HashMap<String, PhpValue>> = RefCell::new(HashMap::new());
    /// This request's legacy operations, in table order.
    static OPERATIONS: RefCell<IndexMap<String, OperationSpec>> = RefCell::new(IndexMap::new());
    /// This request's served table, read from the `wp_xmlrpc_server`.
    static TABLE: RefCell<Option<Vec<MethodEntry>>> = const { RefCell::new(None) };
}

/// Forgets this request's table: a new `wp_xmlrpc_server` starts over, as WordPress's does.
pub fn reset() {
    CALLBACKS.with(|c| c.borrow_mut().clear());
    OPERATIONS.with(|o| o.borrow_mut().clear());
    TABLE.with(|t| *t.borrow_mut() = None);
}

/// This request's legacy operations.
pub fn operations() -> Vec<OperationSpec> {
    OPERATIONS.with(|o| o.borrow().values().cloned().collect())
}

/// The served method table: the one read from the request's `wp_xmlrpc_server`, or, when none was read, `table`.
pub(crate) fn methods(table: Vec<MethodEntry>) -> Vec<MethodEntry> {
    TABLE.with(|t| t.borrow().clone()).unwrap_or(table)
}

fn fnv(text: &str) -> u32 {
    text.bytes().fold(0x811c_9dc5_u32, |h, b| {
        (h ^ u32::from(b)).wrapping_mul(0x0100_0193)
    })
}

/// The operation name of a legacy method: `legacy-xmlrpc/{slug}-{hash}`.
pub fn operation_name(method: &str) -> String {
    let mut slug = String::new();
    for c in method.to_ascii_lowercase().chars() {
        if c.is_ascii_lowercase() || c.is_ascii_digit() {
            slug.push(c);
        } else if !slug.ends_with('-') {
            slug.push('-');
        }
    }
    let slug = slug.trim_matches('-');
    let slug = if slug.is_empty() { "method" } else { slug };
    format!("legacy-xmlrpc/{slug}-{:08x}", fnv(method))
}

fn text(value: &PhpValue) -> String {
    match value {
        PhpValue::Str(s) => s.clone(),
        PhpValue::Int(i) => i.to_string(),
        _ => String::new(),
    }
}

/// Reads the request's method table from its `wp_xmlrpc_server` and registers each legacy entry, with its handler
/// bound in `container` for the request. Returns the problems, for the log.
pub(crate) fn read_table(container: &Rc<Container>) -> Vec<String> {
    let mut problems = Vec::new();
    let Some(PhpValue::Array(rows)) = adapter("table", &[]) else {
        return problems;
    };
    let mut table = Vec::new();
    for (_, row) in rows {
        let PhpValue::Array(row) = row else { continue };
        let [(_, method), (_, kind), (_, payload)] = row.as_slice() else {
            continue;
        };
        let method = text(method);
        if text(kind) == "op" {
            table.push(MethodEntry {
                method,
                operation: text(payload),
                legacy: false,
            });
            continue;
        }
        let name = operation_name(&method);
        let spec = match Operation::command(&name)
            .input::<RawInput>(S::any())
            .output::<RawOutput>(S::any())
            .transports(&[XMLRPC])
            .public()
            .build()
        {
            Ok(spec) => spec,
            Err(err) => {
                problems.push(format!("{method}: {err}"));
                continue;
            }
        };
        OPERATIONS.with(|o| o.borrow_mut().insert(name.clone(), spec));
        CALLBACKS.with(|c| c.borrow_mut().insert(name.clone(), payload.clone()));
        let handler: Rc<dyn OperationHandler> = Rc::new(LegacyHandler {
            operation: name.clone(),
        });
        let _request = container.registering(Lifetime::Request);
        bind_handler(container, &name, move |_, _| Ok(handler.clone()));
        table.push(MethodEntry {
            method,
            operation: name,
            legacy: true,
        });
    }
    TABLE.with(|t| *t.borrow_mut() = Some(table));
    problems
}

fn field<'a>(items: &'a [(Key, PhpValue)], name: &str) -> Option<&'a PhpValue> {
    items
        .iter()
        .find(|(k, _)| *k == Key::Str(name.into()))
        .map(|(_, v)| v)
}

/// A tagged result as an outcome: a tagged fault is an error keeping its fault code.
pub(crate) fn outcome(tagged: &PhpValue) -> Result<Value, OperationError> {
    if let PhpValue::Array(items) = tagged
        && matches!(field(items, "t"), Some(PhpValue::Str(t)) if t == "fault")
    {
        let code = match field(items, "code") {
            Some(PhpValue::Int(code)) => *code,
            _ => 500,
        };
        let message = field(items, "message").map(text).unwrap_or_default();
        return Err(error_of(&Fault::new(code, message)));
    }
    Ok(from_tagged(tagged))
}

/// Runs one legacy method's PHP entry.
pub struct LegacyHandler {
    pub operation: String,
}

impl OperationHandler for LegacyHandler {
    fn handle(
        &self,
        _input: Box<dyn Any>,
        _caller: &Caller,
    ) -> Result<Box<dyn Any>, OperationError> {
        Err(OperationError::new(
            "handler_not_neutral",
            Category::Internal,
            "a legacy XML-RPC handler is neutral",
        ))
    }

    fn neutral(&self) -> bool {
        true
    }

    fn handle_neutral(&self, input: &Value, _caller: &Caller) -> Result<Value, OperationError> {
        let callback = CALLBACKS
            .with(|c| c.borrow().get(&self.operation).cloned())
            .ok_or_else(|| {
                OperationError::new(
                    "xmlrpc_invalid_handler",
                    Category::Internal,
                    format!("{} has no method in this request", self.operation),
                )
            })?;
        let params = match input.get("params") {
            Some(Value::List(params)) => params.clone(),
            _ => Vec::new(),
        };
        let tagged = PhpValue::Array(
            params
                .iter()
                .enumerate()
                .map(|(i, v)| (Key::Int(i as i64), to_tagged(v)))
                .collect(),
        );
        let result = speedyspec_php::call_catching(
            &PhpValue::Str("SpeedySpec\\XmlRpc\\Internal\\Adapter::invoke".into()),
            &[callback, tagged],
        )
        .map_err(|failure| {
            eprintln!(
                "speedyspec_wordpress: the XML-RPC method {} failed: {}",
                self.operation, failure.error
            );
            OperationError::new(
                "xmlrpc_method_failed",
                Category::Internal,
                "The request could not be processed.",
            )
        })?;
        outcome(&result)
    }
}
