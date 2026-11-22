//! WordPress status codes, error bodies, HTTP responses and _fields.

use serde_json::json;
use speedyspec_http::{Message, Response};
use speedyspec_operation_contracts::{Category, OperationError, Value};
use speedyspec_rest::{encode, errors, fields};
use speedyspec_rest_contracts::{RestLink, RestResponse};

fn err(code: &str, category: Category) -> OperationError {
    OperationError::new(code, category, "m")
}

fn body(response: &RestResponse) -> serde_json::Value {
    encode::to_json(&response.data)
}

#[test]
fn categories_map_to_statuses() {
    let cases = [
        (Category::NotFound, 404),
        (Category::PermissionDenied, 403),
        (Category::Unauthenticated, 401),
        (Category::InvalidArgument, 400),
        (Category::AlreadyExists, 409),
        (Category::Conflict, 409),
        (Category::ResourceExhausted, 429),
        (Category::Unavailable, 503),
        (Category::Internal, 500),
    ];
    for (category, status) in cases {
        assert_eq!(
            errors::status_of(&err("x", category)),
            status,
            "{category:?}"
        );
    }
    let overridden =
        err("x", Category::InvalidArgument).with_details(Value::map([("status", Value::Int(422))]));
    assert_eq!(errors::status_of(&overridden), 422);
}

#[test]
fn plain_errors_have_wordpress_bodies() {
    let e = OperationError::new("report_locked", Category::Conflict, "Locked.")
        .with_details(Value::map([("by", Value::Int(2))]));
    let r = errors::error_response(&e);
    assert_eq!(r.status, 409);
    assert_eq!(
        body(&r),
        json!({"code": "report_locked", "message": "Locked.", "data": {"status": 409, "by": 2}})
    );
}

fn violations(list: &[(&str, &str, &str)]) -> OperationError {
    OperationError::new(
        "invalid_input",
        Category::InvalidArgument,
        "invalid input for x",
    )
    .with_details(Value::List(
        list.iter()
            .map(|(path, rule, message)| {
                Value::map([
                    ("path", Value::from(*path)),
                    ("rule", Value::from(*rule)),
                    ("message", Value::from(*message)),
                ])
            })
            .collect(),
    ))
}

#[test]
fn invalid_input_becomes_rest_invalid_param() {
    let r = errors::error_response(&violations(&[
        ("id", "type", "must be an integer"),
        ("tags[0]", "type", "must be a string"),
    ]));
    assert_eq!(r.status, 400);
    assert_eq!(
        body(&r),
        json!({
            "code": "rest_invalid_param",
            "message": "Invalid parameter(s): id, tags",
            "data": {
                "status": 400,
                "params": {"id": "must be an integer", "tags": "must be a string"},
                "details": {
                    "id": {"code": "rest_invalid_param", "message": "must be an integer", "data": null},
                    "tags": {"code": "rest_invalid_param", "message": "must be a string", "data": null}
                }
            }
        })
    );
}

#[test]
fn missing_required_becomes_rest_missing_callback_param() {
    let r = errors::error_response(&violations(&[
        ("a", "required", "is required"),
        ("b", "required", "is required"),
    ]));
    assert_eq!(
        body(&r),
        json!({"code": "rest_missing_callback_param", "message": "Missing parameter(s): a, b", "data": {"status": 400, "params": ["a", "b"]}})
    );
}

#[test]
fn permission_errors_become_rest_forbidden() {
    let anon = errors::error_response(&err("operation_unauthenticated", Category::Unauthenticated));
    let signed = errors::error_response(&err("operation_forbidden", Category::PermissionDenied));
    assert_eq!((anon.status, signed.status), (401, 403));
    assert_eq!(
        body(&signed),
        json!({"code": "rest_forbidden", "message": "Sorry, you are not allowed to do that.", "data": {"status": 403}})
    );
    let missing = errors::error_response(&err("operation_not_found", Category::NotFound));
    assert_eq!(body(&missing)["code"], "rest_no_route");
}

#[test]
fn http_responses_are_json_and_head_has_no_body() {
    let mut r = RestResponse::ok(Value::map([("a", Value::Int(1))]));
    r.status = 201;
    r.headers.insert("X-A".into(), "1".into());
    r.links.push(RestLink {
        rel: "self".into(),
        href: "http://x.test/wp-json/a/v1/b/1".into(),
        embeddable: false,
    });
    let http = encode::http_response(&r, false);
    assert_eq!(http.status().as_u16(), 201);
    assert_eq!(
        http.headers().get("Content-Type").as_deref(),
        Some("application/json; charset=UTF-8")
    );
    assert_eq!(http.headers().get("X-A").as_deref(), Some("1"));
    assert_eq!(
        http.headers().get("Link").as_deref(),
        Some("<http://x.test/wp-json/a/v1/b/1>; rel=\"self\"")
    );
    assert_eq!(http.body().borrow_mut().read_all().unwrap(), br#"{"a":1}"#);
    let head = encode::http_response(&r, true);
    assert!(head.body().borrow_mut().read_all().unwrap().is_empty());
}

#[test]
fn fields_filter_nested_and_lists() {
    let data = encode::from_serde(&json!([
        {"id": 1, "title": "t", "meta": {"a": 1, "b": 2}, "_links": {}},
        {"id": 2, "title": "u", "meta": {"a": 3, "b": 4}}
    ]));
    let filtered = fields::filter(&data, &["id".into(), "meta.a".into()]);
    assert_eq!(
        encode::to_json(&filtered),
        json!([{"id": 1, "meta": {"a": 1}}, {"id": 2, "meta": {"a": 3}}])
    );
    let with_links = fields::filter(&data, &["id".into(), "_links".into()]);
    assert_eq!(
        encode::to_json(&with_links)[0],
        json!({"id": 1, "_links": {}})
    );
}
