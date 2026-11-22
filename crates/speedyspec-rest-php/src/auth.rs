//! Who calls, as WordPress's current user.

use std::rc::Rc;

use speedyspec_http_message::HttpServerRequest;
use speedyspec_operation_contracts::{Caller, Identity, OperationError};
use speedyspec_php_ext::Value as PhpValue;
use speedyspec_rest::REST;
use speedyspec_transport_contracts::Authenticator;

/// The caller is WordPress's current user (`get_current_user_id()`, 0 for nobody), resolved after the server ran
/// `rest_authentication_errors`, as `WP_REST_Server::check_authentication` leaves it.
pub struct PhpRestAuthenticator {
    identity: Rc<dyn Identity>,
}

impl PhpRestAuthenticator {
    pub fn new(identity: Rc<dyn Identity>) -> PhpRestAuthenticator {
        PhpRestAuthenticator { identity }
    }
}

impl Authenticator<HttpServerRequest> for PhpRestAuthenticator {
    fn authenticate(&self, _request: &HttpServerRequest) -> Result<Caller, OperationError> {
        Ok(self.current())
    }
}

impl PhpRestAuthenticator {
    /// WordPress's current user as a REST caller.
    pub fn current(&self) -> Caller {
        let exists = speedyspec_php::call(
            &PhpValue::Str("function_exists".into()),
            &[PhpValue::Str("get_current_user_id".into())],
        );
        let user = if matches!(exists, Ok(PhpValue::Bool(true))) {
            match speedyspec_php::call(&PhpValue::Str("get_current_user_id".into()), &[]) {
                Ok(PhpValue::Int(id)) if id > 0 => Some(id as u64),
                _ => None,
            }
        } else {
            None
        };
        Caller::new(user, REST, self.identity.clone())
    }
}
