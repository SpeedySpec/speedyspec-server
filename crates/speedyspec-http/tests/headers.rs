use speedyspec_http::{Headers, HttpError};

#[test]
fn header_names_are_case_insensitive_and_keep_case() {
    let headers = Headers::new()
        .with("Content-Type", vec!["text/html".into()])
        .unwrap();
    assert_eq!(headers.get("content-type").as_deref(), Some("text/html"));
    let names: Vec<&str> = headers.iter().map(|(name, _)| name).collect();
    assert_eq!(names, vec!["Content-Type"]);
}

#[test]
fn with_replaces_existing_values_whatever_the_case() {
    let headers = Headers::new()
        .with("X-A", vec!["1".into()])
        .unwrap()
        .with("x-a", vec!["2".into()])
        .unwrap();
    assert_eq!(headers.get_all("X-A"), ["2".to_string()]);
}

#[test]
fn with_added_appends() {
    let headers = Headers::new()
        .with("Accept", vec!["a".into()])
        .unwrap()
        .with_added("accept", vec!["b".into()])
        .unwrap();
    assert_eq!(
        headers.get_all("ACCEPT"),
        ["a".to_string(), "b".to_string()]
    );
}

#[test]
fn get_joins_with_comma_space() {
    let headers = Headers::new()
        .with("Accept", vec!["a".into(), "b".into()])
        .unwrap();
    assert_eq!(headers.get("accept").as_deref(), Some("a, b"));
}

#[test]
fn crlf_in_value_is_rejected() {
    let err = Headers::new()
        .with("X-Bad", vec!["a\r\nSet-Cookie: x".into()])
        .unwrap_err();
    assert!(matches!(err, HttpError::InvalidHeaderValue(_)));
}

#[test]
fn invalid_name_is_rejected() {
    let err = Headers::new()
        .with("Bad Name", vec!["a".into()])
        .unwrap_err();
    assert!(matches!(err, HttpError::InvalidHeaderName(_)));
}

#[test]
fn without_removes_every_case() {
    let headers = Headers::new()
        .with("X-A", vec!["1".into()])
        .unwrap()
        .without("x-a");
    assert!(!headers.contains("X-A"));
    assert_eq!(headers.get("X-A"), None);
}
