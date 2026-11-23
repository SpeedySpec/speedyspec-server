//! Logging in with WordPress's `wp_authenticate()`.

use std::rc::Rc;

use speedyspec_operation_contracts::{Caller, Category, Identity, OperationError};
use speedyspec_php_ext::Value as PhpValue;
use speedyspec_xmlrpc::XMLRPC;
use speedyspec_xmlrpc_contracts::XmlRpcLogin;

use crate::convert::adapter;

/// The caller is the user `wp_authenticate()` returns, made WordPress's current user as `wp_xmlrpc_server::login`
/// makes it. A Rust login replaces it once users are ported.
pub struct PhpXmlRpcLogin {
    identity: Rc<dyn Identity>,
}

impl PhpXmlRpcLogin {
    pub fn new(identity: Rc<dyn Identity>) -> PhpXmlRpcLogin {
        PhpXmlRpcLogin { identity }
    }
}

impl XmlRpcLogin for PhpXmlRpcLogin {
    fn authenticate(&self, username: &str, password: &str) -> Result<Caller, OperationError> {
        let args = [
            PhpValue::Str(username.to_owned()),
            PhpValue::Str(password.to_owned()),
        ];
        match adapter("login", &args) {
            Some(PhpValue::Int(id)) if id > 0 => Ok(Caller::new(
                u64::try_from(id).ok(),
                XMLRPC,
                self.identity.clone(),
            )),
            _ => Err(OperationError::new(
                "invalid_login",
                Category::Unauthenticated,
                "Incorrect username or password.",
            )),
        }
    }
}
