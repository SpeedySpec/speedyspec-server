//! REST bindings, requests and responses.

use indexmap::IndexMap;
use speedyspec_operation_contracts::Value;
use speedyspec_rest_contracts::{RestBinding, RestMethod, RestRequest, RestResponse};

fn binding(namespace: &str, route: &str, methods: &[&str]) -> Value {
    Value::map([
        ("namespace", Value::from(namespace)),
        ("route", Value::from(route)),
        (
            "methods",
            Value::List(methods.iter().map(|m| Value::from(*m)).collect()),
        ),
    ])
}

#[test]
fn binding_round_trips_and_validates() {
    let b =
        RestBinding::from_value(&binding("acme/v2", "/r/(?P<id>\\d+)", &["put", "GET"])).unwrap();
    assert_eq!(b.methods, vec![RestMethod::Put, RestMethod::Get]);
    assert_eq!(b.full_route(), "/acme/v2/r/(?P<id>\\d+)");
    assert_eq!(RestBinding::from_value(&b.to_value()).unwrap(), b);
    let err = |v: Value| RestBinding::from_value(&v).unwrap_err();
    assert_eq!(
        err(binding("acme", "/x", &["GET"])),
        "namespace must be vendor/version"
    );
    assert_eq!(
        err(binding("acme/v1", "x", &["GET"])),
        "route must start with /"
    );
    assert_eq!(
        err(binding("acme/v1", "/x", &[])),
        "methods must not be empty"
    );
    assert_eq!(
        err(binding("acme/v1", "/x", &["TRACE"])),
        "unknown method TRACE"
    );
}

#[test]
fn method_parse_is_case_insensitive() {
    assert_eq!(RestMethod::parse("pAtCh"), Some(RestMethod::Patch));
    assert_eq!(RestMethod::parse("head"), None);
    assert_eq!(RestMethod::Delete.as_str(), "DELETE");
}

fn map(entries: &[(&str, &str)]) -> IndexMap<String, Value> {
    entries
        .iter()
        .map(|(k, v)| ((*k).to_owned(), Value::from(*v)))
        .collect()
}

fn request(method: RestMethod) -> RestRequest {
    RestRequest::new(method, "/acme/v1/reports/3")
}

#[test]
fn params_merge_in_trunk_order() {
    // WP_REST_Request::get_parameter_order: JSON (JSON bodies), body (POST/PUT/PATCH/DELETE), query, URL; first wins.
    let mut r = request(RestMethod::Post);
    r.json_body = true;
    r.query_params = map(&[("k", "query"), ("q", "query")]);
    r.body_params = map(&[("k", "body"), ("b", "body")]);
    r.json_params = map(&[("k", "json"), ("j", "json")]);
    r.url_params = map(&[("k", "url"), ("u", "url")]);
    let p = r.params();
    assert_eq!(p.get("k"), Some(&Value::from("json")));
    assert_eq!(p.get("q"), Some(&Value::from("query")));
    assert_eq!(p.get("b"), Some(&Value::from("body")));
    assert_eq!(p.get("u"), Some(&Value::from("url")));
    r.json_params.clear();
    assert_eq!(r.params().get("k"), Some(&Value::from("body")));
    r.body_params.clear();
    assert_eq!(
        r.params().get("k"),
        Some(&Value::from("query")),
        "the query beats URL parameters, as in trunk"
    );
    let mut g = request(RestMethod::Get);
    g.body_params = map(&[("b", "body")]);
    g.url_params = map(&[("id", "3")]);
    assert_eq!(
        g.params().get("b"),
        None,
        "body parameters are ignored on GET"
    );
    assert_eq!(g.params().get("id"), Some(&Value::from("3")));
}

#[test]
fn set_param_updates_every_source_or_the_first_in_order() {
    let mut r = request(RestMethod::Post);
    r.url_params = map(&[("id", "3")]);
    r.query_params = map(&[("id", "3")]);
    r.set_param("id", Value::from("4"));
    assert_eq!(r.url_params.get("id"), Some(&Value::from("4")));
    assert_eq!(r.query_params.get("id"), Some(&Value::from("4")));
    assert_eq!(r.params().get("id"), Some(&Value::from("4")));
    r.set_param("title", Value::from("t"));
    assert_eq!(r.body_params.get("title"), Some(&Value::from("t")));
    r.json_body = true;
    r.set_param("other", Value::from("j"));
    assert_eq!(r.json_params.get("other"), Some(&Value::from("j")));
    let mut g = request(RestMethod::Get);
    g.set_param("page", Value::Int(2));
    assert_eq!(g.query_params.get("page"), Some(&Value::Int(2)));
}

#[test]
fn response_ok_is_200() {
    let r = RestResponse::ok(Value::Null);
    assert_eq!((r.status, r.links.len(), r.headers.len()), (200, 0, 0));
}

#[test]
fn defaults_are_the_lowest_priority() {
    let mut g = request(RestMethod::Get);
    g.default_params = map(&[("a", "d"), ("b", "d")]);
    g.query_params = map(&[("a", "q")]);
    let p = g.params();
    assert_eq!(p.get("a"), Some(&Value::from("q")));
    assert_eq!(p.get("b"), Some(&Value::from("d")));
    g.set_param("c", Value::from("x"));
    assert_eq!(g.query_params.get("c"), Some(&Value::from("x")));
    assert!(!g.default_params.contains_key("c"));
    g.set_param("b", Value::from("y"));
    assert_eq!(g.default_params.get("b"), Some(&Value::from("y")));
    assert_eq!(g.params().get("b"), Some(&Value::from("y")));
}
