//! Operation errors as WordPress REST answers them.

use indexmap::IndexMap;
use speedyspec_operation_contracts::{Category, OperationError, Value};
use speedyspec_rest_contracts::RestResponse;

/// The HTTP status for an error: a numeric `details.status` wins, else its category's.
pub fn status_of(error: &OperationError) -> u16 {
    if let Some(Value::Int(status)) = error.details.get("status")
        && let Ok(status) = u16::try_from(*status)
        && (100..600).contains(&status)
    {
        return status;
    }
    match error.category {
        Category::NotFound => 404,
        Category::PermissionDenied => 403,
        Category::Unauthenticated => 401,
        Category::InvalidArgument => 400,
        Category::AlreadyExists | Category::Conflict => 409,
        Category::ResourceExhausted => 429,
        Category::Unavailable => 503,
        Category::Internal => 500,
    }
}

/// The category for a status a WordPress error carries (the inverse of [`status_of`], as near as statuses allow).
pub fn category_of(status: Option<u16>) -> Category {
    match status {
        Some(401) => Category::Unauthenticated,
        Some(403) => Category::PermissionDenied,
        Some(404) => Category::NotFound,
        Some(409) => Category::Conflict,
        Some(429) => Category::ResourceExhausted,
        Some(503) => Category::Unavailable,
        Some(s) if (400..500).contains(&s) => Category::InvalidArgument,
        _ => Category::Internal,
    }
}

fn body(code: &str, message: &str, data: IndexMap<String, Value>) -> Value {
    Value::map([
        ("code", Value::from(code)),
        ("message", Value::from(message)),
        ("data", Value::Map(data)),
    ])
}

/// The parameter a violation path names: its first segment (`tags[0]` and `meta.a` name `tags` and `meta`).
fn parameter(path: &str) -> String {
    path.split(['.', '[']).next().unwrap_or(path).to_owned()
}

fn invalid_input(error: &OperationError) -> Option<RestResponse> {
    let Value::List(violations) = error.details.as_ref() else {
        return None;
    };
    let text = |v: &Value, key: &str| match v.get(key) {
        Some(Value::String(s)) => s.clone(),
        _ => String::new(),
    };
    let missing: Vec<String> = violations
        .iter()
        .filter(|v| text(v, "rule") == "required")
        .map(|v| parameter(&text(v, "path")))
        .collect();
    let data = if !missing.is_empty() {
        let mut data = IndexMap::new();
        data.insert("status".into(), Value::Int(400));
        data.insert(
            "params".into(),
            Value::List(missing.iter().map(|m| Value::from(m.as_str())).collect()),
        );
        body(
            "rest_missing_callback_param",
            &format!("Missing parameter(s): {}", missing.join(", ")),
            data,
        )
    } else {
        let mut params = IndexMap::new();
        let mut details = IndexMap::new();
        for v in violations {
            let name = parameter(&text(v, "path"));
            if params.contains_key(&name) {
                continue;
            }
            let message = text(v, "message");
            params.insert(name.clone(), Value::from(message.as_str()));
            details.insert(
                name,
                Value::map([
                    ("code", Value::from("rest_invalid_param")),
                    ("message", Value::from(message.as_str())),
                    ("data", Value::Null),
                ]),
            );
        }
        let names: Vec<&str> = params.keys().map(String::as_str).collect();
        let message = format!("Invalid parameter(s): {}", names.join(", "));
        let mut data = IndexMap::new();
        data.insert("status".into(), Value::Int(400));
        data.insert("params".into(), Value::Map(params));
        data.insert("details".into(), Value::Map(details));
        body("rest_invalid_param", &message, data)
    };
    Some(RestResponse {
        status: 400,
        ..RestResponse::ok(data)
    })
}

/// The response for an error (`rest_convert_error_to_response`): `{code, message, data: {status, …details}}`, with the
/// dispatcher's own errors in WordPress's words (`rest_invalid_param`, `rest_missing_callback_param`, `rest_forbidden`,
/// `rest_no_route`).
pub fn error_response(error: &OperationError) -> RestResponse {
    if error.code == "invalid_input"
        && let Some(response) = invalid_input(error)
    {
        return response;
    }
    let status = status_of(error);
    let (code, message) = match error.code.as_str() {
        "operation_forbidden" | "operation_unauthenticated" => {
            ("rest_forbidden", "Sorry, you are not allowed to do that.")
        }
        "operation_not_found" => (
            "rest_no_route",
            "No route was found matching the URL and request method.",
        ),
        code => (code, error.message.as_str()),
    };
    let mut data = IndexMap::new();
    data.insert("status".into(), Value::Int(i64::from(status)));
    if let Value::Map(details) = error.details.as_ref() {
        for (k, v) in details {
            if k != "status" {
                data.insert(k.clone(), v.clone());
            }
        }
    }
    RestResponse {
        status,
        ..RestResponse::ok(body(code, message, data))
    }
}
