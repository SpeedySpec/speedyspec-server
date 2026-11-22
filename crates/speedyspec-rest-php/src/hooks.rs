//! The REST hooks through WordPress's `apply_filters`, with proxies for the request and response.

use std::cell::RefCell;
use std::rc::Rc;

use speedyspec_operation_contracts::{Category, OperationError};
use speedyspec_php_ext::{Key, Value as PhpValue};
use speedyspec_rest_contracts::{RestHooks, RestOutcome, RestResponse, SharedRequest};

use crate::errors::{Described, adapter, describe, to_wp_error};

use crate::{area, handles};

/// PHP's emptiness (`empty()`).
pub(crate) fn empty(value: &PhpValue) -> bool {
    match value.read().as_ref() {
        PhpValue::Null => true,
        PhpValue::Bool(b) => !*b,
        PhpValue::Int(i) => *i == 0,
        PhpValue::Float(f) => *f == 0.0,
        PhpValue::Str(s) => s.is_empty() || s == "0",
        PhpValue::Bytes(b) => b.is_empty() || b == b"0",
        PhpValue::Array(items) => items.is_empty(),
        PhpValue::Opaque(_) => false,
    }
}

/// WordPress's REST hooks, fired with `apply_filters` and WordPress's arguments. The request is a
/// `SpeedySpec\Rest\RequestProxy` (a `WP_REST_Request`) and responses `SpeedySpec\Rest\ResponseProxy` (a
/// `WP_REST_Response`), reading and writing the Rust objects live. The server argument is `null` until the
/// `WP_REST_Server` shim exists. Where `apply_filters` does not exist, every hook does nothing.
#[derive(Default)]
pub struct PhpRestHooks;

impl PhpRestHooks {
    pub fn new() -> PhpRestHooks {
        PhpRestHooks
    }
}

/// What a filter call gave: `Ok(None)` when WordPress's hooks are not loaded, `Err` when a callback threw.
type Applied = Result<Option<PhpValue>, OperationError>;

fn apply(hook: &str, mut args: Vec<PhpValue>) -> Applied {
    let exists = speedyspec_php::call(
        &PhpValue::Str("function_exists".into()),
        &[PhpValue::Str("apply_filters".into())],
    );
    if !matches!(exists, Ok(PhpValue::Bool(true))) {
        return Ok(None);
    }
    args.insert(0, PhpValue::Str(hook.to_owned()));
    speedyspec_php::call_catching(&PhpValue::Str("apply_filters".into()), &args)
        .map(Some)
        .map_err(|failure| {
            // As in WordPress, where an uncaught exception ends the request: never let it through. The details go to
            // the log, not the client.
            eprintln!(
                "speedyspec_wordpress: the filter {hook} failed: {}",
                failure.error
            );
            OperationError::new(
                "rest_hook_failed",
                Category::Internal,
                "The request could not be processed.",
            )
        })
}

/// The `WP_REST_Server` of this request (the shim), as WordPress passes it to its hooks; `null` before one exists.
fn server() -> PhpValue {
    adapter("server", &[]).unwrap_or(PhpValue::Null)
}

/// The matched route's handler, as WordPress passes it to its callbacks' hooks: a legacy route's handler array, or an
/// empty array for an operation.
fn matched_handler(request: &SharedRequest) -> PhpValue {
    let operation = request.borrow().operation.clone();
    operation
        .as_deref()
        .and_then(crate::legacy::handler)
        .unwrap_or(PhpValue::Array(vec![]))
}

pub(crate) fn request_proxy(request: &SharedRequest) -> PhpValue {
    area::ensure_proxies();
    let handle = handles::add_request(request.clone());
    adapter("request", &[PhpValue::Int(handle as i64)]).unwrap_or(PhpValue::Null)
}

/// A `WP_REST_Response` (a `SpeedySpec\Rest\ResponseProxy`) holding `response`'s data, status, headers and links.
/// Hooks change it with any of WordPress's methods or properties; [`response_back`] reads it afterwards.
fn response_proxy(response: &RestResponse) -> PhpValue {
    area::ensure_proxies();
    let handle = handles::add_response(Rc::new(RefCell::new(response.clone())));
    adapter("response", &[PhpValue::Int(handle as i64)]).unwrap_or(PhpValue::Null)
}

/// The response a hook left: what it returned (the proxy it was given, read back, or another response), or, for data
/// alone, `original` with that data.
fn response_back(returned: Option<PhpValue>, original: &RestResponse) -> RestResponse {
    match returned.as_ref().map(describe) {
        None | Some(Described::Nothing) => original.clone(),
        Some(Described::Response(r)) => r,
        Some(Described::Data(d)) => RestResponse {
            data: d,
            ..original.clone()
        },
        Some(Described::Error(e)) => speedyspec_rest::errors::error_response(&e),
    }
}

fn outcome_of(described: Described) -> Option<RestOutcome> {
    match described {
        Described::Nothing => None,
        Described::Error(e) => Some(Err(e)),
        Described::Response(r) => Some(Ok(r)),
        Described::Data(d) => Some(Ok(RestResponse::ok(d))),
    }
}

impl RestHooks for PhpRestHooks {
    fn authentication_errors(&self) -> Result<(), OperationError> {
        match apply("rest_authentication_errors", vec![PhpValue::Null])?
            .as_ref()
            .map(describe)
        {
            Some(Described::Error(e)) => Err(e),
            _ => Ok(()),
        }
    }

    fn endpoints(&self, routes: Vec<String>) -> Vec<String> {
        let table = PhpValue::Array(
            routes
                .iter()
                .map(|r| (Key::Str(r.clone()), PhpValue::Array(vec![])))
                .collect(),
        );
        match apply("rest_endpoints", vec![table]) {
            Ok(Some(PhpValue::Array(items))) => items
                .into_iter()
                .filter_map(|(k, _)| match k {
                    Key::Str(route) => Some(route),
                    Key::Int(_) => None,
                })
                .collect(),
            _ => routes,
        }
    }

    fn pre_dispatch(&self, request: &SharedRequest) -> Option<RestOutcome> {
        let result = match apply(
            "rest_pre_dispatch",
            vec![PhpValue::Null, server(), request_proxy(request)],
        ) {
            Err(error) => return Some(Err(error)),
            Ok(result) => result?,
        };
        if empty(&result) {
            return None;
        }
        outcome_of(describe(&result))
    }

    fn before_callbacks(&self, request: &SharedRequest) -> Result<(), OperationError> {
        let args = vec![
            PhpValue::Null,
            matched_handler(request),
            request_proxy(request),
        ];
        match apply("rest_request_before_callbacks", args)?
            .as_ref()
            .map(describe)
        {
            Some(Described::Error(e)) => Err(e),
            _ => Ok(()),
        }
    }

    fn dispatch_request(&self, request: &SharedRequest) -> Option<RestOutcome> {
        let route = request
            .borrow()
            .route
            .clone()
            .map_or(PhpValue::Null, PhpValue::Str);
        let args = vec![
            PhpValue::Null,
            request_proxy(request),
            route,
            matched_handler(request),
        ];
        match apply("rest_dispatch_request", args) {
            Err(error) => Some(Err(error)),
            Ok(None) => None,
            Ok(Some(result)) => outcome_of(describe(&result)),
        }
    }

    fn prepare_item(
        &self,
        type_name: &str,
        response: RestResponse,
        request: &SharedRequest,
    ) -> RestResponse {
        let proxy = response_proxy(&response);
        match apply(
            &format!("rest_prepare_{type_name}"),
            vec![proxy, PhpValue::Null, request_proxy(request)],
        ) {
            Err(_) => response,
            Ok(returned) => match returned.as_ref().map(describe) {
                Some(Described::Error(_)) => response,
                _ => response_back(returned, &response),
            },
        }
    }

    fn after_callbacks(&self, outcome: RestOutcome, request: &SharedRequest) -> RestOutcome {
        let value = match &outcome {
            Ok(response) => response_proxy(response),
            Err(error) => to_wp_error(error),
        };
        let returned = match apply(
            "rest_request_after_callbacks",
            vec![value, matched_handler(request), request_proxy(request)],
        ) {
            Err(error) => return Err(error),
            Ok(None) => return outcome,
            Ok(Some(returned)) => returned,
        };
        match describe(&returned) {
            Described::Error(e) => Err(e),
            Described::Response(r) => Ok(r),
            Described::Data(d) => Ok(match &outcome {
                Ok(original) => RestResponse {
                    data: d,
                    ..original.clone()
                },
                Err(_) => RestResponse::ok(d),
            }),
            // `rest_ensure_response( null )`.
            Described::Nothing => Ok(RestResponse::ok(
                speedyspec_operation_contracts::Value::Null,
            )),
        }
    }

    fn post_dispatch(&self, response: RestResponse, request: &SharedRequest) -> RestResponse {
        let proxy = response_proxy(&response);
        match apply(
            "rest_post_dispatch",
            vec![proxy, server(), request_proxy(request)],
        ) {
            Err(error) => speedyspec_rest::errors::error_response(&error),
            Ok(returned) => response_back(returned, &response),
        }
    }

    fn pre_serve(&self, response: &RestResponse, request: &SharedRequest) -> bool {
        let proxy = response_proxy(response);
        matches!(
            apply("rest_pre_serve_request", vec![PhpValue::Bool(false), proxy, request_proxy(request), server()]),
            Ok(Some(v)) if !empty(&v)
        )
    }
}
