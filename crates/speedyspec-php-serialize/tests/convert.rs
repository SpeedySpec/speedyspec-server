use std::rc::Rc;

use speedyspec_hooks::{Key, Value};
use speedyspec_php_serialize::{
    ConvertError, OpaquePhpValue, PhpValue, from_hook_value, to_hook_value, unserialize,
};

#[test]
fn scalars_and_arrays_round_trip() {
    let v = Value::Array(vec![
        (Key::Str("a".into()), Value::Int(1)),
        (Key::Int(0), Value::Str("x".into())),
    ]);
    let php = from_hook_value(&v).unwrap();
    assert_eq!(to_hook_value(&php), v);
    assert_eq!(
        to_hook_value(&from_hook_value(&Value::Float(1.5)).unwrap()),
        Value::Float(1.5)
    );
}

#[test]
fn objects_become_opaque_and_return() {
    let php = unserialize(b"O:8:\"stdClass\":0:{}").unwrap();
    let hook = to_hook_value(&php);
    match &hook {
        Value::Opaque(any) => assert!(any.downcast_ref::<OpaquePhpValue>().is_some()),
        other => panic!("{other:?}"),
    }
    assert_eq!(from_hook_value(&hook).unwrap(), php);
}

#[test]
fn object_ref_shares_rc() {
    let php = unserialize(b"a:2:{i:0;O:8:\"stdClass\":0:{}i:1;r:2;}").unwrap();
    match to_hook_value(&php) {
        Value::Array(items) => match (&items[0].1, &items[1].1) {
            (Value::Opaque(a), Value::Opaque(b)) => assert!(Rc::ptr_eq(a, b)),
            other => panic!("{other:?}"),
        },
        other => panic!("{other:?}"),
    }
}

#[test]
fn value_ref_is_copied() {
    let php = unserialize(b"a:2:{i:0;i:1;i:1;R:2;}").unwrap();
    assert_eq!(
        to_hook_value(&php),
        Value::Array(vec![
            (Key::Int(0), Value::Int(1)),
            (Key::Int(1), Value::Int(1))
        ])
    );
}

#[test]
fn cycles_and_odd_keys_stay_whole() {
    let cycle = unserialize(b"a:1:{i:0;R:1;}").unwrap();
    match to_hook_value(&cycle) {
        Value::Opaque(any) => assert_eq!(any.downcast_ref::<OpaquePhpValue>().unwrap().0, cycle),
        other => panic!("{other:?}"),
    }
    let odd = unserialize(b"a:1:{s:1:\"\xff\";i:1;}").unwrap();
    assert!(matches!(to_hook_value(&odd), Value::Opaque(_)));
}

#[test]
fn foreign_opaque_refused() {
    assert!(matches!(
        from_hook_value(&Value::Opaque(Rc::new(5u8))),
        Err(ConvertError::Foreign)
    ));
}

#[test]
fn invalid_utf8_string_is_bytes() {
    assert_eq!(
        to_hook_value(&PhpValue::Str(vec![0xff, b'a'])),
        Value::Bytes(vec![0xff, b'a'])
    );
    assert_eq!(
        from_hook_value(&Value::Bytes(vec![0xff])).unwrap(),
        PhpValue::Str(vec![0xff])
    );
}
