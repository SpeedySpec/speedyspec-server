//! The REST hooks through WordPress's `apply_filters`, with proxies for the request and response.

use std::cell::RefCell;
use std::rc::Rc;

use speedyspec_operation_contracts::OperationError;
use speedyspec_php_ext::{Key, Value as PhpValue};
use speedyspec_rest_contracts::{RestHooks, RestOutcome, RestResponse, SharedRequest};

use crate::errors::{Described, adapter, describe, to_wp_error};
use crate::{area, handles};

/// PHP's emptiness (`empty()`).
fn empty(value: &PhpValue) -> bool {
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

fn apply(hook: &str, mut args: Vec<PhpValue>) -> Option<PhpValue> {
    let exists = speedyspec_php::call(
        &PhpValue::Str("function_exists".into()),
        &[PhpValue::Str("apply_filters".into())],
    );
    if !matches!(exists, Ok(PhpValue::Bool(true))) {
        return None;
    }
    args.insert(0, PhpValue::Str(hook.to_owned()));
    speedyspec_php::call_catching(&PhpValue::Str("apply_filters".into()), &args)
        .map_err(|failure| {
            eprintln!(
                "speedyspec_wordpress: the filter {hook} failed: {}",
                failure.error
            )
        })
        .ok()
}

fn request_proxy(request: &SharedRequest) -> PhpValue {
    area::ensure_proxies();
    let handle = handles::add_request(request.clone());
    adapter("request", &[PhpValue::Int(handle as i64)]).unwrap_or(PhpValue::Null)
}

fn response_proxy(response: &RestResponse) -> (PhpValue, handles::SharedResponse) {
    area::ensure_proxies();
    let shared = Rc::new(RefCell::new(response.clone()));
    let handle = handles::add_response(shared.clone());
    let proxy = adapter("response", &[PhpValue::Int(handle as i64)]).unwrap_or(PhpValue::Null);
    (proxy, shared)
}

/// The response a hook left: what it returned, or (for data alone) the proxied response with that data.
fn response_back(returned: Option<PhpValue>, shared: &handles::SharedResponse) -> RestResponse {
    match returned.as_ref().map(describe) {
        None | Some(Described::Nothing) => shared.borrow().clone(),
        Some(Described::Response(r)) => r,
        Some(Described::Data(d)) => RestResponse {
            data: d,
            ..shared.borrow().clone()
        },
        Some(Described::Error(e)) => speedyspec_rest::errors::error_response(&e),
    }
}

impl RestHooks for PhpRestHooks {
    fn authentication_errors(&self) -> Result<(), OperationError> {
        match apply("rest_authentication_errors", vec![PhpValue::Null])
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
            Some(PhpValue::Array(items)) => items
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
        let result = apply(
            "rest_pre_dispatch",
            vec![PhpValue::Null, PhpValue::Null, request_proxy(request)],
        )?;
        if empty(&result) {
            return None;
        }
        crate::errors::from_php(&result)
    }

    fn before_callbacks(&self, request: &SharedRequest) -> Result<(), OperationError> {
        let args = vec![
            PhpValue::Null,
            PhpValue::Array(vec![]),
            request_proxy(request),
        ];
        match apply("rest_request_before_callbacks", args)
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
        let result = apply(
            "rest_dispatch_request",
            vec![
                PhpValue::Null,
                request_proxy(request),
                route,
                PhpValue::Array(vec![]),
            ],
        )?;
        match describe(&result) {
            Described::Nothing => None,
            Described::Error(e) => Some(Err(e)),
            Described::Response(r) => Some(Ok(r)),
            Described::Data(d) => Some(Ok(RestResponse::ok(d))),
        }
    }

    fn prepare_item(
        &self,
        type_name: &str,
        response: RestResponse,
        request: &SharedRequest,
    ) -> RestResponse {
        let (proxy, shared) = response_proxy(&response);
        let returned = apply(
            &format!("rest_prepare_{type_name}"),
            vec![proxy, PhpValue::Null, request_proxy(request)],
        );
        match returned.as_ref().map(describe) {
            Some(Described::Error(_)) => response,
            _ => response_back(returned, &shared),
        }
    }

    fn after_callbacks(&self, outcome: RestOutcome, request: &SharedRequest) -> RestOutcome {
        let (value, shared) = match &outcome {
            Ok(response) => {
                let (proxy, shared) = response_proxy(response);
                (proxy, Some(shared))
            }
            Err(error) => (to_wp_error(error), None),
        };
        let Some(returned) = apply(
            "rest_request_after_callbacks",
            vec![value, PhpValue::Array(vec![]), request_proxy(request)],
        ) else {
            return outcome;
        };
        match describe(&returned) {
            Described::Error(e) => Err(e),
            Described::Response(r) => Ok(r),
            Described::Data(d) => Ok(match &shared {
                Some(shared) => RestResponse {
                    data: d,
                    ..shared.borrow().clone()
                },
                None => RestResponse::ok(d),
            }),
            // `rest_ensure_response( null )`.
            Described::Nothing => Ok(RestResponse::ok(
                speedyspec_operation_contracts::Value::Null,
            )),
        }
    }

    fn post_dispatch(&self, response: RestResponse, request: &SharedRequest) -> RestResponse {
        let (proxy, shared) = response_proxy(&response);
        let returned = apply(
            "rest_post_dispatch",
            vec![proxy, PhpValue::Null, request_proxy(request)],
        );
        response_back(returned, &shared)
    }

    fn pre_serve(&self, response: &RestResponse, request: &SharedRequest) -> bool {
        let (proxy, _) = response_proxy(response);
        apply(
            "rest_pre_serve_request",
            vec![
                PhpValue::Bool(false),
                proxy,
                request_proxy(request),
                PhpValue::Null,
            ],
        )
        .is_some_and(|v| !empty(&v))
    }
}
