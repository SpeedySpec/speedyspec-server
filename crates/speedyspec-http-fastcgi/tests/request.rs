use std::io::Cursor;

use speedyspec_http::{Message, Method, Request, ServerRequest, Uri};
use speedyspec_http_fastcgi::server_request_from_params;

fn params(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

#[test]
fn builds_uri_from_params() {
    let p = params(&[
        ("REQUEST_METHOD", "POST"),
        ("HTTPS", "on"),
        ("HTTP_HOST", "example.com:8443"),
        ("SERVER_PORT", "8443"),
        ("REQUEST_URI", "/blog/a%20b?x=1"),
    ]);
    let request = server_request_from_params(&p, Box::new(Cursor::new(Vec::new())), 1024).unwrap();
    assert_eq!(request.method(), &Method::Post);
    assert_eq!(
        request.uri().to_uri_string(),
        "https://example.com:8443/blog/a%20b?x=1"
    );
}

#[test]
fn server_name_used_without_host_header() {
    let p = params(&[
        ("REQUEST_METHOD", "GET"),
        ("SERVER_NAME", "a.test"),
        ("SERVER_PORT", "80"),
        ("REQUEST_URI", "/"),
    ]);
    let request = server_request_from_params(&p, Box::new(Cursor::new(Vec::new())), 1024).unwrap();
    assert_eq!(request.uri().to_uri_string(), "http://a.test/");
}

#[test]
fn http_headers_are_mapped() {
    let p = params(&[
        ("REQUEST_METHOD", "GET"),
        ("HTTP_HOST", "a.test"),
        ("REQUEST_URI", "/"),
        ("HTTP_X_FORWARDED_FOR", "203.0.113.7"),
        ("CONTENT_TYPE", "text/plain"),
    ]);
    let request = server_request_from_params(&p, Box::new(Cursor::new(Vec::new())), 1024).unwrap();
    assert_eq!(
        request.headers().get("X-Forwarded-For").as_deref(),
        Some("203.0.113.7")
    );
    assert!(
        request
            .headers()
            .iter()
            .any(|(name, _)| name == "X-Forwarded-For")
    );
    assert_eq!(
        request.headers().get("content-type").as_deref(),
        Some("text/plain")
    );
}

#[test]
fn cookies_split_like_php() {
    let p = params(&[
        ("REQUEST_METHOD", "GET"),
        ("HTTP_HOST", "a.test"),
        ("REQUEST_URI", "/"),
        ("HTTP_COOKIE", "a=1; b=x%20y; c"),
    ]);
    let request = server_request_from_params(&p, Box::new(Cursor::new(Vec::new())), 1024).unwrap();
    assert_eq!(
        request.cookie_params(),
        params(&[("a", "1"), ("b", "x y"), ("c", "")]).as_slice()
    );
}

#[test]
fn query_params_are_flat_and_decoded() {
    let p = params(&[
        ("REQUEST_METHOD", "GET"),
        ("HTTP_HOST", "a.test"),
        ("REQUEST_URI", "/?a=1&b=x+y"),
        ("QUERY_STRING", "a=1&b=x+y"),
    ]);
    let request = server_request_from_params(&p, Box::new(Cursor::new(Vec::new())), 1024).unwrap();
    assert_eq!(
        request.query_params(),
        params(&[("a", "1"), ("b", "x y")]).as_slice()
    );
}

#[test]
fn server_params_keep_order() {
    let p = params(&[
        ("REQUEST_METHOD", "GET"),
        ("Z", "1"),
        ("A", "2"),
        ("HTTP_HOST", "a.test"),
        ("REQUEST_URI", "/"),
    ]);
    let request = server_request_from_params(&p, Box::new(Cursor::new(Vec::new())), 1024).unwrap();
    assert_eq!(request.server_params(), p.as_slice());
}

#[test]
fn body_is_lazy() {
    let p = params(&[
        ("REQUEST_METHOD", "POST"),
        ("HTTP_HOST", "a.test"),
        ("REQUEST_URI", "/"),
    ]);
    let request =
        server_request_from_params(&p, Box::new(Cursor::new(b"payload".to_vec())), 1024).unwrap();
    assert_eq!(request.body().borrow_mut().read_all().unwrap(), b"payload");
}
