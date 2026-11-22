//! The neutral value and operation errors.

use indexmap::IndexMap;
use speedyspec_operation_contracts::{Category, OperationError, Value};

#[test]
fn map_keeps_insertion_order_and_get_reads_it() {
    let v = Value::map([("b", Value::from(1i64)), ("a", Value::from("x"))]);
    let Value::Map(m) = &v else { panic!() };
    assert_eq!(m.keys().collect::<Vec<_>>(), ["b", "a"]);
    assert_eq!(v.get("a"), Some(&Value::String("x".into())));
    assert_eq!(Value::from(1i64).get("a"), None);
}

#[test]
fn kind_names() {
    assert_eq!(Value::Null.kind_name(), "null");
    assert_eq!(Value::Float(1.5).kind_name(), "number");
    assert_eq!(Value::Binary(vec![1]).kind_name(), "binary");
    assert_eq!(Value::Map(IndexMap::new()).kind_name(), "object");
}

#[test]
fn categories_use_canonical_names() {
    let all = [
        (Category::NotFound, "not_found"),
        (Category::PermissionDenied, "permission_denied"),
        (Category::Unauthenticated, "unauthenticated"),
        (Category::InvalidArgument, "invalid_argument"),
        (Category::AlreadyExists, "already_exists"),
        (Category::Conflict, "conflict"),
        (Category::ResourceExhausted, "resource_exhausted"),
        (Category::Unavailable, "unavailable"),
        (Category::Internal, "internal"),
    ];
    for (c, s) in all {
        assert_eq!(c.as_str(), s);
    }
}

#[test]
fn error_displays_code_and_message_and_carries_details() {
    let e = OperationError::new("post_not_found", Category::NotFound, "No post 7.")
        .with_details(Value::map([("id", Value::from(7i64))]));
    assert_eq!(e.to_string(), "post_not_found: No post 7.");
    assert_eq!(e.details.get("id"), Some(&Value::Int(7)));
}

#[test]
fn category_parse_inverts_as_str() {
    use speedyspec_operation_contracts::Category;
    let all = [
        Category::NotFound,
        Category::PermissionDenied,
        Category::Unauthenticated,
        Category::InvalidArgument,
        Category::AlreadyExists,
        Category::Conflict,
        Category::ResourceExhausted,
        Category::Unavailable,
        Category::Internal,
    ];
    for c in all {
        assert_eq!(Category::parse(c.as_str()), Some(c));
    }
    assert_eq!(Category::parse("teapot"), None);
}
