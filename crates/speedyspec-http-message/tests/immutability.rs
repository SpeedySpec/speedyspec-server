use speedyspec_http::{Message, Method, Request, Response, ServerRequest, StatusCode};
use speedyspec_http::{ResponseFactory, ServerRequestFactory};
use speedyspec_http_message::{HttpResponse, HttpServerRequest, MessageFactory};

#[test]
fn with_header_leaves_original() {
    let factory = MessageFactory;
    let original: HttpResponse = factory.create_response(StatusCode::new(200).unwrap(), "");
    let changed = original
        .clone()
        .with_header("X-A", vec!["1".into()])
        .unwrap();
    assert!(!original.headers().contains("X-A"));
    assert!(changed.headers().contains("X-A"));
    assert_eq!(changed.reason_phrase(), "OK");
}

#[test]
fn server_request_attributes_and_target() {
    let factory = MessageFactory;
    let request: HttpServerRequest = factory
        .create_server_request(
            Method::Get,
            "http://example.com/a?b=1",
            vec![("X".into(), "y".into())],
        )
        .unwrap();
    assert_eq!(request.request_target(), "/a?b=1");
    assert_eq!(
        request.server_params(),
        [("X".to_string(), "y".to_string())]
    );
    let request = request.with_attribute(7u8);
    assert_eq!(request.attributes().get::<u8>(), Some(&7));
    let request = request.with_method(Method::Post);
    assert_eq!(request.method(), &Method::Post);
}
