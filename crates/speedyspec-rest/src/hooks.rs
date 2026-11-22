//! REST hooks that do nothing.

use speedyspec_operation_contracts::OperationError;
use speedyspec_rest_contracts::{RestHooks, RestOutcome, RestResponse, SharedRequest};

/// No hooks: nothing short-circuits, nothing changes. The default where WordPress's hooks are not loaded.
pub struct NoRestHooks;

impl RestHooks for NoRestHooks {
    fn authentication_errors(&self) -> Result<(), OperationError> {
        Ok(())
    }

    fn endpoints(&self, routes: Vec<String>) -> Vec<String> {
        routes
    }

    fn pre_dispatch(&self, _: &SharedRequest) -> Option<RestOutcome> {
        None
    }

    fn before_callbacks(&self, _: &SharedRequest) -> Result<(), OperationError> {
        Ok(())
    }

    fn dispatch_request(&self, _: &SharedRequest) -> Option<RestOutcome> {
        None
    }

    fn prepare_item(&self, _: &str, response: RestResponse, _: &SharedRequest) -> RestResponse {
        response
    }

    fn after_callbacks(&self, outcome: RestOutcome, _: &SharedRequest) -> RestOutcome {
        outcome
    }

    fn post_dispatch(&self, response: RestResponse, _: &SharedRequest) -> RestResponse {
        response
    }

    fn pre_serve(&self, _: &RestResponse, _: &SharedRequest) -> bool {
        false
    }
}
