use speedyspec_http::Uri;
use speedyspec_http_message::HttpUri;

#[test]
fn uri_normalizes_like_psr7() {
    let uri: HttpUri = "HTTP://Example.COM:80/a".parse().unwrap();
    assert_eq!(uri.scheme(), "http");
    assert_eq!(uri.host(), "example.com");
    assert_eq!(uri.port(), None);
    assert_eq!(uri.to_uri_string(), "http://example.com/a");
}

#[test]
fn non_default_port_is_kept() {
    let uri: HttpUri = "https://example.com:8443/x?y=1#f".parse().unwrap();
    assert_eq!(uri.port(), Some(8443));
    assert_eq!(uri.authority(), "example.com:8443");
    assert_eq!(uri.query(), "y=1");
    assert_eq!(uri.fragment(), "f");
    assert_eq!(uri.to_uri_string(), "https://example.com:8443/x?y=1#f");
}

#[test]
fn rootless_path_with_authority_gets_a_slash() {
    let uri: HttpUri = "http://example.com".parse().unwrap();
    assert_eq!(uri.with_path("b").to_uri_string(), "http://example.com/b");
}

#[test]
fn path_is_encoded_once_never_twice() {
    let uri: HttpUri = "http://example.com".parse().unwrap();
    assert_eq!(uri.with_path("/a b%20").path(), "/a%20b%20");
}

#[test]
fn user_info_is_part_of_authority() {
    let uri: HttpUri = "http://example.com/".parse().unwrap();
    let uri = uri.with_user_info("u", Some("p"));
    assert_eq!(uri.user_info(), "u:p");
    assert_eq!(uri.authority(), "u:p@example.com");
}
