use speedyspec_http::{
    Method, Request, RequestFactory, Response, ResponseFactory, StatusCode, StreamFactory,
    UploadedFile, UploadedFileFactory, Uri, UriFactory,
};
use speedyspec_http_message::{
    HttpRequest, HttpResponse, HttpUploadedFile, HttpUri, MessageFactory,
};

#[test]
fn factory_creates_each_kind() {
    let factory = MessageFactory;
    let request: HttpRequest = factory
        .create_request(Method::Put, "http://example.com/x")
        .unwrap();
    assert_eq!(request.uri().path(), "/x");
    let response: HttpResponse = factory.create_response(StatusCode::new(418).unwrap(), "Teapot");
    assert_eq!(response.reason_phrase(), "Teapot");
    let uri: HttpUri = factory.create_uri("https://a.test/").unwrap();
    assert_eq!(uri.host(), "a.test");
    let mut stream = factory.create_stream(b"x");
    assert_eq!(stream.read_all().unwrap(), b"x");
    let file: HttpUploadedFile = factory.create_uploaded_file(
        factory.create_stream(b"data"),
        Some(4),
        0,
        Some("a.txt"),
        Some("text/plain"),
    );
    assert_eq!(file.client_filename(), Some("a.txt"));
    assert_eq!(file.size(), Some(4));
}
