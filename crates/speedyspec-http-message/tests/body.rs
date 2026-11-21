use std::io::Cursor;

use speedyspec_http::{Body, Headers, HttpError, Message, Response, StatusCode};
use speedyspec_http_message::{BytesBody, HttpResponse, LazyBody, Replay, SentRecord};

#[test]
fn lazy_body_unread_is_handed_to_php_unread() {
    let body = LazyBody::new(Box::new(Cursor::new(b"abc".to_vec())), 10);
    match body.take_for_php() {
        Replay::Unread(mut reader) => {
            let mut bytes = Vec::new();
            reader.read_to_end(&mut bytes).unwrap();
            assert_eq!(bytes, b"abc");
        }
        Replay::Buffered(_) => panic!("an unread body must stay unread"),
    }
}

#[test]
fn lazy_body_read_is_replayed_byte_for_byte() {
    let mut body = LazyBody::new(Box::new(Cursor::new(vec![0u8, 255, 10, 13])), 10);
    assert_eq!(body.read_all().unwrap(), vec![0u8, 255, 10, 13]);
    assert_eq!(body.read_all().unwrap(), vec![0u8, 255, 10, 13]);
    match body.take_for_php() {
        Replay::Buffered(bytes) => assert_eq!(bytes, vec![0u8, 255, 10, 13]),
        Replay::Unread(_) => panic!("a read body must be replayed"),
    }
}

#[test]
fn lazy_body_over_limit_errors() {
    let mut body = LazyBody::new(Box::new(Cursor::new(vec![1u8; 11])), 10);
    assert!(matches!(body.read_all(), Err(HttpError::Io(_))));
}

#[test]
fn sent_body_cannot_be_read() {
    let record = SentRecord {
        status: StatusCode::new(200).unwrap(),
        headers: Headers::new().with("X-A", vec!["1".into()]).unwrap(),
    };
    let response = HttpResponse::sent(record);
    assert!(response.body().borrow().is_sent());
    assert_eq!(
        response.body().borrow_mut().read_all(),
        Err(HttpError::BodyAlreadySent)
    );
    assert_eq!(response.status().as_u16(), 200);
    assert!(response.headers().contains("x-a"));
}

#[test]
fn bytes_body_size() {
    let mut body = BytesBody::new(b"hey".to_vec());
    assert_eq!(body.size(), Some(3));
    assert_eq!(body.read_all().unwrap(), b"hey");
}

use std::io::Read;
