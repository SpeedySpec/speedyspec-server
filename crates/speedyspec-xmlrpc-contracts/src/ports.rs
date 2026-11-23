//! The XML-RPC transport's ports.

use speedyspec_operation_contracts::{
    Caller, Category, OperationError, OperationSpec, TypeCatalog, Value,
};

use crate::{Fault, MethodEntry, XmlRpcBinding};

/// How an operation is named and called when it has no explicit binding (Strategy).
pub trait XmlRpcMapping {
    /// The operation's method, or `None` when it has no default one.
    fn binding(&self, spec: &OperationSpec, types: &dyn TypeCatalog) -> Option<XmlRpcBinding>;
}

/// Which fault code an error category is sent with (Strategy).
pub trait FaultMapping {
    fn code(&self, category: Category) -> i64;
}

/// Turns the username and password in a call into a caller.
pub trait XmlRpcLogin {
    fn authenticate(&self, username: &str, password: &str) -> Result<Caller, OperationError>;
}

/// WordPress's XML-RPC hooks.
pub trait XmlRpcHooks {
    /// Whether XML-RPC methods that log in are enabled (`pre_option_enable_xmlrpc`, `option_enable_xmlrpc`,
    /// `xmlrpc_enabled`).
    fn enabled(&self) -> bool;
    /// The element limit of one request (`xmlrpc_element_limit`).
    fn element_limit(&self, default: usize) -> usize;
    /// The served method table (`xmlrpc_methods`): the exposed operations in, the table out. Adapters may add legacy
    /// operations, replace entries and remove them.
    fn methods(&self, table: Vec<MethodEntry>) -> Vec<MethodEntry>;
    /// Fired after login and before an operation method runs (`xmlrpc_call`).
    fn call(&self, method: &str, params: &[Value]) -> Result<(), OperationError>;
    /// The fault a failed login is sent with (`xmlrpc_login_error`).
    fn login_error(&self, fault: Fault, username: &str) -> Fault;
}

/// WordPress's defaults, with no hooks: enabled, the default limit, the table unchanged, nothing fired.
#[derive(Default)]
pub struct NoXmlRpcHooks;

impl XmlRpcHooks for NoXmlRpcHooks {
    fn enabled(&self) -> bool {
        true
    }

    fn element_limit(&self, default: usize) -> usize {
        default
    }

    fn methods(&self, table: Vec<MethodEntry>) -> Vec<MethodEntry> {
        table
    }

    fn call(&self, _method: &str, _params: &[Value]) -> Result<(), OperationError> {
        Ok(())
    }

    fn login_error(&self, fault: Fault, _username: &str) -> Fault {
        fault
    }
}
