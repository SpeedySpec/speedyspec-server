//! HTTP requests to REST requests.

use indexmap::IndexMap;
use serde_json::Value as Json;
use speedyspec_http::{Message, Method, Request, Uri};
use speedyspec_http_message::HttpServerRequest;
use speedyspec_operation_contracts::Value;
use speedyspec_rest_contracts::{RestMethod, RestRequest, header_key};

use crate::query;

/// The REST URL prefix.
pub const PREFIX: &str = "/wp-json";

/// A `serde_json` value as a neutral value.
pub fn from_json(json: &Json) -> Value {
    match json {
        Json::Null => Value::Null,
        Json::Bool(b) => Value::Bool(*b),
        Json::Number(n) => n
            .as_i64()
            .map_or_else(|| Value::Float(n.as_f64().unwrap_or(f64::NAN)), Value::Int),
        Json::String(s) => Value::String(s.clone()),
        Json::Array(items) => Value::List(items.iter().map(from_json).collect()),
        Json::Object(map) => {
            Value::Map(map.iter().map(|(k, v)| (k.clone(), from_json(v))).collect())
        }
    }
}

/// The REST request `request` is, or `None` when it is not one: its path is not under `/wp-json` and its query has no
/// `rest_route`, or its method is not one REST serves (`OPTIONS` is not served yet).
pub fn rest_request(request: &HttpServerRequest) -> Option<RestRequest> {
    let uri = request.uri();
    let raw_query = uri.query().to_owned();
    let mut query_params = query::parse(&raw_query);
    let decoded = query::decode_path(uri.path());
    let path = match decoded.as_str() {
        p if p == PREFIX => "/".to_owned(),
        p if p.starts_with("/wp-json/") => {
            let rest = &p[PREFIX.len()..];
            if rest == "/" {
                "/".to_owned()
            } else {
                rest.to_owned()
            }
        }
        _ => match query_params.shift_remove("rest_route") {
            Some(Value::String(route)) => route,
            _ => return None,
        },
    };
    let headers: IndexMap<String, Vec<String>> = request
        .headers()
        .iter()
        .map(|(name, values)| (header_key(name), values.to_vec()))
        .collect();
    let override_method = match query_params.get("_method") {
        Some(Value::String(m)) => Some(m.clone()),
        _ => headers
            .get("x_http_method_override")
            .and_then(|v| v.first())
            .cloned(),
    };
    let (method, head) = match override_method {
        Some(name) => (RestMethod::parse(&name)?, false),
        None => match request.method() {
            Method::Head => (RestMethod::Get, true),
            other => (RestMethod::parse(other.as_str())?, false),
        },
    };
    let body = request.body().borrow_mut().read_all().unwrap_or_default();
    let content_type = headers
        .get("content_type")
        .and_then(|v| v.first())
        .map(|c| c.to_ascii_lowercase())
        .unwrap_or_default();
    let mut rest = RestRequest::new(method, &path);
    rest.head = head;
    if content_type.contains("application/json") {
        rest.json_body = true;
        if let Ok(Json::Object(map)) = serde_json::from_slice::<Json>(&body) {
            rest.json_params = map.iter().map(|(k, v)| (k.clone(), from_json(v))).collect();
        }
    } else if content_type.contains("application/x-www-form-urlencoded") {
        rest.body_params = query::parse(&String::from_utf8_lossy(&body));
    }
    rest.query_params = query_params;
    rest.headers = headers;
    rest.body = body;
    Some(rest)
}
