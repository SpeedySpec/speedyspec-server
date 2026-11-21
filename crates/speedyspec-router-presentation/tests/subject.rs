use speedyspec_http::{Method, ServerRequestFactory};
use speedyspec_http_message::{HttpServerRequest, MessageFactory};
use speedyspec_router_presentation::wp_subject;

fn request(params: &[(&str, &str)]) -> HttpServerRequest {
    let params = params
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    MessageFactory
        .create_server_request(Method::Get, "http://example.test/", params)
        .unwrap()
}

fn subject(params: &[(&str, &str)], home_path: &str) -> Option<(String, String)> {
    wp_subject(&request(params), home_path, "index.php")
        .map(|s| (s.requested_path, s.requested_file))
}

#[test]
fn requested_path_matches_parse_request() {
    let root = [
        ("SCRIPT_NAME", "/index.php"),
        ("PHP_SELF", "/index.php"),
        ("REQUEST_URI", "/2024/05/hello/?x=1"),
    ];
    assert_eq!(
        subject(&root, ""),
        Some(("2024/05/hello".into(), "2024/05/hello".into()))
    );

    let sub = [
        ("SCRIPT_NAME", "/blog/index.php"),
        ("PHP_SELF", "/blog/index.php"),
        ("REQUEST_URI", "/blog/about/"),
    ];
    assert_eq!(
        subject(&sub, "/blog"),
        Some(("about".into(), "about".into()))
    );

    let info = [
        ("SCRIPT_NAME", "/index.php"),
        ("PHP_SELF", "/index.php/2024/01"),
        ("PATH_INFO", "/2024/01"),
        ("REQUEST_URI", "/index.php/2024/01"),
    ];
    assert_eq!(
        subject(&info, ""),
        Some(("2024/01".into(), "index.php".into()))
    );

    let index = [
        ("SCRIPT_NAME", "/index.php"),
        ("PHP_SELF", "/index.php"),
        ("REQUEST_URI", "/index.php"),
    ];
    assert_eq!(subject(&index, ""), Some(("".into(), "".into())));
}

#[test]
fn other_scripts_have_no_wordpress_subject() {
    let login = [
        ("SCRIPT_NAME", "/wp-login.php"),
        ("PHP_SELF", "/wp-login.php"),
        ("REQUEST_URI", "/wp-login.php"),
    ];
    assert_eq!(subject(&login, ""), None);
}
