//! Decoding HTTP requests the way WordPress reads them.

use std::rc::Rc;

use speedyspec_http::{Message, Method, ServerRequestFactory, StreamFactory};
use speedyspec_http_message::{HttpServerRequest, MessageFactory};
use speedyspec_operation::S;
use speedyspec_operation_contracts::{TypeCatalog, TypeSpec, Value};
use speedyspec_rest::{coerce, decode, query};
use speedyspec_rest_contracts::RestMethod;

fn request(method: Method, uri: &str) -> HttpServerRequest {
    MessageFactory
        .create_server_request(method, uri, vec![])
        .unwrap()
}

fn with_body(r: HttpServerRequest, content_type: &str, body: &str) -> HttpServerRequest {
    r.with_header("Content-Type", vec![content_type.into()])
        .unwrap()
        .with_body(MessageFactory.create_stream(body.as_bytes()))
}

#[test]
fn only_rest_paths_are_decoded() {
    let path = |uri: &str| decode::rest_request(&request(Method::Get, uri)).map(|r| r.path);
    assert_eq!(
        path("http://x.test/wp-json/acme/v1/reports").as_deref(),
        Some("/acme/v1/reports")
    );
    assert_eq!(
        path("http://x.test/?rest_route=/acme/v1/reports").as_deref(),
        Some("/acme/v1/reports")
    );
    assert_eq!(path("http://x.test/about"), None);
    assert_eq!(path("http://x.test/wp-json").as_deref(), Some("/"));
    assert_eq!(path("http://x.test/wp-json/").as_deref(), Some("/"));
    assert_eq!(path("http://x.test/wp-jsonish"), None);
}

#[test]
fn method_override() {
    let r = decode::rest_request(&request(
        Method::Post,
        "http://x.test/wp-json/a/v1/b?_method=DELETE",
    ))
    .unwrap();
    assert_eq!(r.method, RestMethod::Delete);
    let h = request(Method::Post, "http://x.test/wp-json/a/v1/b")
        .with_header("X-HTTP-Method-Override", vec!["PUT".into()])
        .unwrap();
    assert_eq!(decode::rest_request(&h).unwrap().method, RestMethod::Put);
    let head =
        decode::rest_request(&request(Method::Head, "http://x.test/wp-json/a/v1/b")).unwrap();
    assert_eq!((head.method, head.head), (RestMethod::Get, true));
}

#[test]
fn php_style_query() {
    let q = query::parse("a[]=1&a[]=2&m[b]=3&m[c]=4&s=x+y%20z&k=1&k=2");
    assert_eq!(
        q.get("a"),
        Some(&Value::List(vec![Value::from("1"), Value::from("2")]))
    );
    assert_eq!(
        q.get("m"),
        Some(&Value::map([
            ("b", Value::from("3")),
            ("c", Value::from("4"))
        ]))
    );
    assert_eq!(q.get("s"), Some(&Value::from("x y z")));
    assert_eq!(q.get("k"), Some(&Value::from("2")));
    let r = decode::rest_request(&request(
        Method::Get,
        "http://x.test/wp-json/a/v1/b?page=2&tags[]=x",
    ))
    .unwrap();
    assert_eq!(r.query_params.get("page"), Some(&Value::from("2")));
    assert_eq!(
        r.query_params.get("tags"),
        Some(&Value::List(vec![Value::from("x")]))
    );
}

#[test]
fn bodies_fill_their_params() {
    let json = with_body(
        request(Method::Post, "http://x.test/wp-json/a/v1/b"),
        "application/json; charset=utf-8",
        r#"{"title": "t", "n": 5}"#,
    );
    let r = decode::rest_request(&json).unwrap();
    assert_eq!(r.json_params.get("n"), Some(&Value::Int(5)));
    assert!(r.json_body);
    let list = with_body(
        request(Method::Post, "http://x.test/wp-json/a/v1/b"),
        "application/json",
        "[1,2]",
    );
    assert!(decode::rest_request(&list).unwrap().json_params.is_empty());
    let form = with_body(
        request(Method::Post, "http://x.test/wp-json/a/v1/b"),
        "application/x-www-form-urlencoded",
        "title=t&tags[]=a",
    );
    let r = decode::rest_request(&form).unwrap();
    assert_eq!(r.body_params.get("title"), Some(&Value::from("t")));
    assert_eq!(
        r.header("content-type"),
        Some("application/x-www-form-urlencoded")
    );
    assert_eq!(r.body, b"title=t&tags[]=a");
}

struct NoTypes;

impl TypeCatalog for NoTypes {
    fn get_type(&self, _: &str) -> Option<Rc<TypeSpec>> {
        None
    }
    fn type_names(&self) -> Vec<String> {
        vec![]
    }
}

#[test]
fn coerce_query_strings() {
    let schema = S::object([
        ("id", S::integer()),
        ("price", S::number()),
        ("flag", S::boolean()),
        ("tags", S::array(S::string())),
        ("ids", S::array(S::integer())),
        ("name", S::string()),
    ])
    .build();
    let c = |pairs: &[(&str, Value)]| {
        coerce::coerce(
            &schema,
            &Value::map(pairs.iter().map(|(k, v)| (*k, v.clone()))),
            &NoTypes,
        )
    };
    let out = c(&[
        ("id", Value::from("5")),
        ("price", Value::from("2.5")),
        ("flag", Value::from("false")),
        ("tags", Value::from("a,b")),
        ("ids", Value::List(vec![Value::from("1"), Value::from("2")])),
        ("name", Value::from("5")),
    ]);
    assert_eq!(out.get("id"), Some(&Value::Int(5)));
    assert_eq!(out.get("price"), Some(&Value::Float(2.5)));
    assert_eq!(out.get("flag"), Some(&Value::Bool(false)));
    assert_eq!(
        out.get("tags"),
        Some(&Value::List(vec![Value::from("a"), Value::from("b")]))
    );
    assert_eq!(
        out.get("ids"),
        Some(&Value::List(vec![Value::Int(1), Value::Int(2)]))
    );
    assert_eq!(out.get("name"), Some(&Value::from("5")));
    for (raw, expected) in [("0", false), ("", false), ("true", true), ("1", true)] {
        assert_eq!(
            c(&[("flag", Value::from(raw))]).get("flag"),
            Some(&Value::Bool(expected)),
            "{raw:?}"
        );
    }
    assert_eq!(
        c(&[("id", Value::from("abc"))]).get("id"),
        Some(&Value::from("abc"))
    );
    assert_eq!(
        c(&[("flag", Value::from("maybe"))]).get("flag"),
        Some(&Value::from("maybe"))
    );
}
