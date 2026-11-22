//! WordPress values (WP_Error, WP_REST_Response, plain data) to and from REST outcomes.

use indexmap::IndexMap;
use speedyspec_operation_contracts::{OperationError, Value};
use speedyspec_operation_php::convert::{from_php as plain, to_php};
use speedyspec_php_ext::{Key, Value as PhpValue};
use speedyspec_rest::errors::{category_of, status_of};
use speedyspec_rest_contracts::{RestOutcome, RestResponse};

use crate::handles;

const ADAPTER: &str = "SpeedySpec\\Rest\\Internal\\Adapter::";

pub(crate) fn adapter(method: &str, args: &[PhpValue]) -> Option<PhpValue> {
    speedyspec_php::call(&PhpValue::Str(format!("{ADAPTER}{method}")), args)
        .map_err(|err| eprintln!("speedyspec_wordpress: the REST adapter's {method} failed: {err}"))
        .ok()
}

/// What a PHP value a hook returned is.
pub(crate) enum Described {
    Nothing,
    Error(OperationError),
    Response(RestResponse),
    Data(Value),
}

fn field<'a>(items: &'a [(Key, PhpValue)], name: &str) -> Option<&'a PhpValue> {
    items
        .iter()
        .find(|(k, _)| *k == Key::Str(name.into()))
        .map(|(_, v)| v)
}

fn text(value: Option<&PhpValue>) -> String {
    match value {
        Some(PhpValue::Str(s)) => s.clone(),
        Some(PhpValue::Int(i)) => i.to_string(),
        _ => String::new(),
    }
}

/// The error a `WP_Error`'s parts describe: its `status` gives the category; its data, `status` included, the details.
pub fn wp_error(code: &str, message: &str, data: Value) -> OperationError {
    let status = match data.get("status") {
        Some(Value::Int(s)) => u16::try_from(*s).ok(),
        Some(Value::String(s)) => s.parse().ok(),
        _ => None,
    };
    let details = match data {
        Value::Map(map) => Value::Map(map),
        Value::Null => Value::Null,
        other => Value::map([("data", other)]),
    };
    OperationError::new(code, category_of(status), message).with_details(details)
}

pub(crate) fn describe(value: &PhpValue) -> Described {
    let Some(PhpValue::Array(items)) = adapter("describe", std::slice::from_ref(value)) else {
        return Described::Nothing;
    };
    let data = || {
        field(&items, "data")
            .map(plain)
            .transpose()
            .ok()
            .flatten()
            .unwrap_or(Value::Null)
    };
    match text(field(&items, "kind")).as_str() {
        "error" => Described::Error(wp_error(
            &text(field(&items, "code")),
            &text(field(&items, "message")),
            data(),
        )),
        "response" => {
            if let Some(PhpValue::Int(handle)) = field(&items, "handle")
                && let Some(shared) = handles::response(*handle as u64)
            {
                return Described::Response(shared.borrow().clone());
            }
            let status = match field(&items, "status") {
                Some(PhpValue::Int(s)) => u16::try_from(*s).unwrap_or(200),
                _ => 200,
            };
            let headers: IndexMap<String, String> = match field(&items, "headers") {
                Some(PhpValue::Array(h)) => h
                    .iter()
                    .filter_map(|(k, v)| match (k, v) {
                        (Key::Str(k), PhpValue::Str(v)) => Some((k.clone(), v.clone())),
                        _ => None,
                    })
                    .collect(),
                _ => IndexMap::new(),
            };
            Described::Response(RestResponse {
                status,
                headers,
                ..RestResponse::ok(data())
            })
        }
        "value" => Described::Data(data()),
        _ => Described::Nothing,
    }
}

/// A value a hook returned, as an outcome: a `WP_Error` an error (by its `status`), a `WP_REST_Response` (or its proxy)
/// a response, anything else data with status 200; `null` nothing.
pub fn from_php(value: &PhpValue) -> Option<RestOutcome> {
    match describe(value) {
        Described::Nothing => None,
        Described::Error(e) => Some(Err(e)),
        Described::Response(r) => Some(Ok(r)),
        Described::Data(d) => Some(Ok(RestResponse::ok(d))),
    }
}

/// `error` as a `WP_Error`, in WordPress's own words: the code, message and data its REST response would carry
/// (`rest_invalid_param` with its `params` and `details`, `rest_forbidden`, …), `status` included.
pub fn to_wp_error(error: &OperationError) -> PhpValue {
    let body = speedyspec_rest::errors::error_response(error).data;
    let text = |key: &str| match body.get(key) {
        Some(Value::String(s)) => s.clone(),
        _ => String::new(),
    };
    let data = match body.get("data") {
        Some(Value::Map(map)) => map.clone(),
        _ => {
            let mut data = IndexMap::new();
            data.insert("status".into(), Value::Int(i64::from(status_of(error))));
            data
        }
    };
    adapter(
        "error",
        &[
            PhpValue::Str(text("code")),
            PhpValue::Str(text("message")),
            to_php(&Value::Map(data)),
        ],
    )
    .unwrap_or(PhpValue::Null)
}
