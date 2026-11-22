//! Neutral values to and from PHP values.

use chrono::NaiveDate;
use speedyspec_operation_contracts::{Category, Value};
use speedyspec_operation_php::convert::{from_php, to_php};
use speedyspec_php_ext::{Key, Value as PhpValue};

#[test]
fn dates_cross_as_iso_strings() {
    let dt = NaiveDate::from_ymd_opt(2026, 11, 22)
        .unwrap()
        .and_hms_opt(10, 0, 0)
        .unwrap();
    assert_eq!(
        to_php(&Value::DateTime(dt)),
        PhpValue::Str("2026-11-22T10:00:00".into())
    );
}

#[test]
fn binary_crosses_as_bytes() {
    assert_eq!(
        to_php(&Value::Binary(vec![0xff])),
        PhpValue::Bytes(vec![0xff])
    );
    assert_eq!(
        from_php(&PhpValue::Bytes(vec![0xff])).unwrap(),
        Value::Binary(vec![0xff])
    );
}

#[test]
fn arrays_follow_the_phase_2_rules() {
    assert_eq!(
        to_php(&Value::List(vec![Value::from("a")])),
        PhpValue::Array(vec![(Key::Int(0), PhpValue::Str("a".into()))])
    );
    assert_eq!(
        to_php(&Value::map([("k", Value::Int(1))])),
        PhpValue::Array(vec![(Key::Str("k".into()), PhpValue::Int(1))])
    );
    let list = PhpValue::Array(vec![
        (Key::Int(0), PhpValue::Int(1)),
        (Key::Int(1), PhpValue::Int(2)),
    ]);
    assert_eq!(
        from_php(&list).unwrap(),
        Value::List(vec![Value::Int(1), Value::Int(2)])
    );
    let map = PhpValue::Array(vec![
        (Key::Int(1), PhpValue::Int(1)),
        (Key::Str("x".into()), PhpValue::Null),
    ]);
    assert_eq!(
        from_php(&map).unwrap(),
        Value::map([("1", Value::Int(1)), ("x", Value::Null)])
    );
    assert_eq!(
        from_php(&PhpValue::Array(vec![])).unwrap(),
        Value::List(vec![])
    );
}

#[test]
fn objects_are_refused() {
    let e = from_php(&PhpValue::Opaque(std::rc::Rc::new(1_u8))).unwrap_err();
    assert_eq!(
        (e.code.as_str(), e.category),
        ("php_object_not_allowed", Category::Internal)
    );
}
