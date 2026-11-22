//! The REST hooks port.

use speedyspec_operation_contracts::OperationError;

use crate::{RestResponse, SharedRequest};

/// What a hook point answers instead of the call: a response, or an error.
pub type RestOutcome = Result<RestResponse, OperationError>;

/// WordPress's REST hooks, one method per hook, in the order the server fires them. Each is an integration point: the
/// REST server takes any implementation (a Rust no-op, the PHP adapter, a recording test double).
pub trait RestHooks {
    /// `rest_authentication_errors`: an error refuses the request.
    fn authentication_errors(&self) -> Result<(), OperationError>;
    /// `rest_endpoints`: the route patterns served; returning fewer removes routes.
    fn endpoints(&self, routes: Vec<String>) -> Vec<String>;
    /// `rest_pre_dispatch`: `Some` answers the request before route matching.
    fn pre_dispatch(&self, request: &SharedRequest) -> Option<RestOutcome>;
    /// `rest_request_before_callbacks`, before the permission check: an error skips the permission check and the
    /// handler. The request may be changed, and the handler sees the changes.
    fn before_callbacks(&self, request: &SharedRequest) -> Result<(), OperationError>;
    /// `rest_dispatch_request`, after the permission check: `Some` answers instead of the handler.
    fn dispatch_request(&self, request: &SharedRequest) -> Option<RestOutcome>;
    /// `rest_prepare_{type_name}` on one item of a resource type.
    fn prepare_item(
        &self,
        type_name: &str,
        response: RestResponse,
        request: &SharedRequest,
    ) -> RestResponse;
    /// `rest_request_after_callbacks`: sees the outcome, data or error, and may replace it.
    fn after_callbacks(&self, outcome: RestOutcome, request: &SharedRequest) -> RestOutcome;
    /// `rest_post_dispatch`: the response (errors already encoded as responses).
    fn post_dispatch(&self, response: RestResponse, request: &SharedRequest) -> RestResponse;
    /// `rest_pre_serve_request`: true when a hook served the response itself.
    fn pre_serve(&self, response: &RestResponse, request: &SharedRequest) -> bool;
}
