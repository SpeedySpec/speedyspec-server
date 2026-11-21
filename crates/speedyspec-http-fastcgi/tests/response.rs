use speedyspec_http::{Headers, HttpError, Message, StatusCode};
use speedyspec_http_fastcgi::response_to_cgi;
use speedyspec_http_message::{BytesBody, HttpResponse, SentRecord};

#[test]
fn writes_cgi_status_and_headers() {
    let response = HttpResponse::new(StatusCode::new(301).unwrap())
        .with_header("Location", vec!["/new".into()])
        .unwrap()
        .with_body(Box::new(BytesBody::new(b"moved".to_vec())));
    let bytes = response_to_cgi(response).unwrap();
    assert_eq!(
        bytes,
        b"Status: 301 Moved Permanently\r\nLocation: /new\r\n\r\nmoved".to_vec()
    );
}

#[test]
fn sent_response_is_refused() {
    let response = HttpResponse::sent(SentRecord {
        status: StatusCode::new(200).unwrap(),
        headers: Headers::new(),
    });
    assert_eq!(response_to_cgi(response), Err(HttpError::BodyAlreadySent));
}
