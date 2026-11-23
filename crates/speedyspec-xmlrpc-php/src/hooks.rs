//! WordPress's XML-RPC hooks, through `apply_filters` and `do_action`.

use speedyspec_operation_contracts::{Category, OperationError, Value};
use speedyspec_php_ext::{Key, Value as PhpValue};
use speedyspec_xmlrpc_contracts::{Fault, MethodEntry, XmlRpcHooks};

use crate::convert::{adapter, to_tagged};

/// Whether WordPress's hook functions are loaded.
fn wordpress_loaded() -> bool {
    matches!(
        speedyspec_php::call(
            &PhpValue::Str("function_exists".into()),
            &[PhpValue::Str("apply_filters".into())],
        ),
        Ok(PhpValue::Bool(true))
    )
}

/// Calls the adapter, catching what a hook throws: `Err` when it threw (logged, never sent to the client).
fn guarded(method: &str, args: &[PhpValue]) -> Result<PhpValue, OperationError> {
    speedyspec_php::call_catching(
        &PhpValue::Str(format!("SpeedySpec\\XmlRpc\\Internal\\Adapter::{method}")),
        args,
    )
    .map_err(|failure| {
        eprintln!(
            "speedyspec_wordpress: an XML-RPC hook failed in {method}: {}",
            failure.error
        );
        OperationError::new(
            "xmlrpc_hook_failed",
            Category::Internal,
            "The request could not be processed.",
        )
    })
}

/// WordPress's XML-RPC hooks. Where WordPress's hooks are not loaded, each answers WordPress's default. A hook that
/// throws fails closed: XML-RPC counts as disabled, and the call fails.
#[derive(Default)]
pub struct PhpXmlRpcHooks;

impl PhpXmlRpcHooks {
    pub fn new() -> PhpXmlRpcHooks {
        PhpXmlRpcHooks
    }
}

impl XmlRpcHooks for PhpXmlRpcHooks {
    fn enabled(&self) -> bool {
        if !wordpress_loaded() {
            return true;
        }
        matches!(guarded("enabled", &[]), Ok(PhpValue::Bool(true)))
    }

    fn element_limit(&self, default: usize) -> usize {
        if !wordpress_loaded() {
            return default;
        }
        let as_php = i64::try_from(default).unwrap_or(i64::MAX);
        match guarded("element_limit", &[PhpValue::Int(as_php)]) {
            Ok(PhpValue::Int(limit)) => usize::try_from(limit).unwrap_or(default),
            _ => default,
        }
    }

    fn methods(&self, table: Vec<MethodEntry>) -> Vec<MethodEntry> {
        crate::legacy::methods(table)
    }

    fn call(&self, method: &str, params: &[Value]) -> Result<(), OperationError> {
        if !wordpress_loaded() {
            return Ok(());
        }
        let args = PhpValue::Array(
            params
                .iter()
                .enumerate()
                .map(|(i, v)| (Key::Int(i as i64), to_tagged(v)))
                .collect(),
        );
        guarded("call", &[PhpValue::Str(method.into()), args]).map(drop)
    }

    fn login_error(&self, fault: Fault, _username: &str) -> Fault {
        if !wordpress_loaded() {
            return fault;
        }
        let args = [
            PhpValue::Int(fault.code),
            PhpValue::Str(fault.message.clone()),
        ];
        match adapter("login_error", &args) {
            Some(PhpValue::Array(items)) => match items.as_slice() {
                [(_, PhpValue::Int(code)), (_, PhpValue::Str(message))] => {
                    Fault::new(*code, message.clone())
                }
                _ => fault,
            },
            _ => fault,
        }
    }
}
