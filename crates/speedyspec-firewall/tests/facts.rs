use std::net::{IpAddr, Ipv4Addr};

use speedyspec_firewall::RequestFacts;

fn params(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

#[test]
fn facts_from_params_splits_uri_and_detects_auth_cookie() {
    let facts = RequestFacts::from_params(&params(&[
        ("REMOTE_ADDR", "192.0.2.4"),
        ("REQUEST_METHOD", "GET"),
        ("REQUEST_URI", "/a/b?x=1"),
        ("HTTP_USER_AGENT", "curl/8"),
        ("HTTP_COOKIE", "foo=1; wordpress_logged_in_abc=v"),
    ]))
    .unwrap();
    assert_eq!(facts.client, IpAddr::V4(Ipv4Addr::new(192, 0, 2, 4)));
    assert_eq!(facts.method, "GET");
    assert_eq!(facts.path, "/a/b");
    assert_eq!(facts.query, "x=1");
    assert_eq!(facts.user_agent, "curl/8");
    assert!(facts.has_auth);
}

#[test]
fn facts_detect_authorization_header() {
    let facts = RequestFacts::from_params(&params(&[
        ("REMOTE_ADDR", "192.0.2.4"),
        ("REQUEST_URI", "/"),
        ("HTTP_AUTHORIZATION", "Basic eA=="),
    ]))
    .unwrap();
    assert!(facts.has_auth);
}

#[test]
fn facts_without_cookie_or_authorization_are_anonymous() {
    let facts = RequestFacts::from_params(&params(&[
        ("REMOTE_ADDR", "192.0.2.4"),
        ("REQUEST_URI", "/"),
        ("HTTP_COOKIE", "my_wordpress_logged_in_x=1"),
    ]))
    .unwrap();
    assert!(!facts.has_auth);
    assert_eq!(facts.query, "");
}

#[test]
fn facts_without_remote_addr_is_an_error() {
    assert!(RequestFacts::from_params(&params(&[("REQUEST_URI", "/")])).is_err());
    assert!(
        RequestFacts::from_params(&params(&[("REMOTE_ADDR", "nope"), ("REQUEST_URI", "/")]))
            .is_err()
    );
}
