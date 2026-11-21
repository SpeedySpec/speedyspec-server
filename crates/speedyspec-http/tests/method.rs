use speedyspec_http::{HttpError, Method, StatusCode};

#[test]
fn method_parse_is_case_sensitive() {
    assert_eq!(Method::parse("GET"), Method::Get);
    assert_eq!(Method::parse("get"), Method::Other("get".into()));
    assert_eq!(Method::parse("PURGE").as_str(), "PURGE");
    assert_eq!(Method::Patch.as_str(), "PATCH");
}

#[test]
fn status_rejects_99_and_600() {
    assert!(matches!(
        StatusCode::new(99),
        Err(HttpError::InvalidStatus(99))
    ));
    assert!(matches!(
        StatusCode::new(600),
        Err(HttpError::InvalidStatus(600))
    ));
    assert_eq!(StatusCode::new(404).unwrap().reason(), "Not Found");
    assert_eq!(StatusCode::new(299).unwrap().reason(), "");
    assert_eq!(StatusCode::new(301).unwrap().as_u16(), 301);
}
