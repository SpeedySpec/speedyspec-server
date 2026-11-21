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
    assert!(
        !facts.has_rest_credentials,
        "a logged-in cookie without a nonce is not REST credentials"
    );
}

fn credentials(extra: &[(&str, &str)]) -> bool {
    let mut pairs = vec![
        ("REMOTE_ADDR", "192.0.2.4"),
        ("REQUEST_URI", "/wp-json/wp/v2/users"),
    ];
    pairs.extend_from_slice(extra);
    RequestFacts::from_params(&params(&pairs))
        .unwrap()
        .has_rest_credentials
}

#[test]
fn basic_auth_or_cookie_with_nonce_is_rest_credentials() {
    assert!(credentials(&[("HTTP_AUTHORIZATION", "Basic eA==")]));
    assert!(credentials(&[
        ("HTTP_COOKIE", "wordpress_logged_in_a=1"),
        ("HTTP_X_WP_NONCE", "abc")
    ]));
    let with_query_nonce = RequestFacts::from_params(&params(&[
        ("REMOTE_ADDR", "192.0.2.4"),
        ("REQUEST_URI", "/wp-json/wp/v2/users?_wpnonce=abc"),
        ("HTTP_COOKIE", "wordpress_logged_in_a=1"),
    ]))
    .unwrap();
    assert!(with_query_nonce.has_rest_credentials);
}

#[test]
fn bearer_bare_cookie_or_nonce_alone_is_not_rest_credentials() {
    assert!(!credentials(&[("HTTP_AUTHORIZATION", "Bearer x")]));
    assert!(!credentials(&[("HTTP_COOKIE", "wordpress_logged_in_a=1")]));
    assert!(!credentials(&[("HTTP_X_WP_NONCE", "abc")]));
    assert!(!credentials(&[
        ("HTTP_COOKIE", "my_wordpress_logged_in_x=1"),
        ("HTTP_X_WP_NONCE", "abc")
    ]));
    assert!(!credentials(&[]));
}

#[test]
fn facts_without_remote_addr_is_an_error() {
    assert!(RequestFacts::from_params(&params(&[("REQUEST_URI", "/")])).is_err());
    assert!(
        RequestFacts::from_params(&params(&[("REMOTE_ADDR", "nope"), ("REQUEST_URI", "/")]))
            .is_err()
    );
}
