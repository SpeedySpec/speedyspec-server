//! `xmlrpc.php` over HTTP through `XmlRpcService`: WordPress's methods, served by the Rust codec.

mod support;

use speedyspec_http::{Message, Method, Response, ServerRequestFactory, StreamFactory};
use speedyspec_http_message::MessageFactory;
use speedyspec_php_ext::Value as PhpValue;
use support::*;

const SERVER_CLASS: &str = r#"
class Acme_Server extends wp_xmlrpc_server {
    public function __construct() { $GLOBALS['acme_server'] = true; parent::__construct(); }
}
function acme_server_class( $class ) { return 'Acme_Server'; }
add_filter( 'wp_xmlrpc_server_class', 'acme_server_class' );
"#;

fn post(b: &Booted, body: &str) -> (u16, String, String) {
    let request = MessageFactory
        .create_server_request(Method::Post, "http://x.test/xmlrpc.php", vec![])
        .unwrap()
        .with_body(MessageFactory.create_stream(body.as_bytes()));
    let response = b.service.serve(&request).expect("an XML-RPC server");
    let bytes = response.body().borrow_mut().read_all().unwrap();
    (
        response.status().as_u16(),
        response.headers().get("Content-Type").unwrap_or_default(),
        String::from_utf8(bytes).unwrap(),
    )
}

fn main() {
    let b = boot();
    load_wordpress(&b.php);
    load(&b.php, SERVER_CLASS);

    let (status, content_type, body) = post(
        &b,
        "<?xml version=\"1.0\"?><methodCall><methodName>demo.addTwoNumbers</methodName><params><param><value><int>2</int></value></param><param><value><int>3</int></value></param></params></methodCall>",
    );
    check(
        "demo_add_two_numbers_over_http",
        status == 200 && content_type == "text/xml; charset=UTF-8" && body.contains("<int>5</int>"),
        (&status, &content_type, &body),
    );
    check(
        "wp_xmlrpc_server_class_filter_is_honored",
        global(&b.php, "acme_server") == PhpValue::Bool(true),
        global(&b.php, "acme_server"),
    );

    let (_, _, body) = post(
        &b,
        "<methodCall><methodName>demo.sayHello</methodName></methodCall>",
    );
    check(
        "demo_say_hello_over_http",
        body.contains("<string>Hello!</string>"),
        &body,
    );

    let (_, _, body) = post(
        &b,
        "<methodCall><methodName>demo.addTwoNumbers</methodName><params><param><value><int>2</int></value></param></params></methodCall>",
    );
    check(
        "a_wordpress_fault_over_http",
        body.contains("<int>400</int>") && body.contains("Requires two integers."),
        &body,
    );

    let request = MessageFactory
        .create_server_request(Method::Get, "http://x.test/xmlrpc.php", vec![])
        .unwrap();
    let response = b.service.serve(&request).expect("an XML-RPC server");
    check(
        "get_is_refused",
        response.status().as_u16() == 405,
        response.status().as_u16(),
    );
}
