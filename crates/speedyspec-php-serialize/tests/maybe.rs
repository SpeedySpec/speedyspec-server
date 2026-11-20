use speedyspec_php_serialize::{
    PhpKey, PhpValue, is_serialized, maybe_serialize, maybe_unserialize,
};

fn s(b: &[u8]) -> PhpValue {
    PhpValue::Str(b.to_vec())
}

#[test]
fn is_serialized_strict() {
    for yes in [
        "N;",
        "b:0;",
        "i:5;",
        "d:0.1;",
        "s:0:\"\";",
        "a:0:{}",
        "O:8:\"stdClass\":0:{}",
        "E:7:\"Foo:Bar\";",
        "  i:5;  ",
        "\ni:5;\0",
    ] {
        assert!(is_serialized(yes.as_bytes(), true), "{yes:?}");
    }
    for no in [
        "N",
        "i:5",
        "s:4:test;",
        "a:0:",
        "x:1;",
        "",
        "i:x;",
        "s:3:\"abc",
        "b:1;x",
    ] {
        assert!(!is_serialized(no.as_bytes(), true), "{no:?}");
    }
}

#[test]
fn is_serialized_loose() {
    // Not strict: the value need not end the string.
    assert!(is_serialized(b"s:3:\"abc\";x", false));
    assert!(!is_serialized(b"s:3:\"abc\";x", true));
    assert!(is_serialized(b"i:5;trailing", false));
    assert!(!is_serialized(b"s:3:\"abc", false));
    assert!(!is_serialized(b"a:1;", false), "semicolon too early");
}

#[test]
fn maybe_serialize_rules() {
    assert_eq!(maybe_serialize(&PhpValue::Array(vec![])), s(b"a:0:{}"));
    assert_eq!(maybe_serialize(&s(b"i:5;")), s(b"s:4:\"i:5;\";"));
    assert_eq!(maybe_serialize(&s(b"plain")), s(b"plain"));
    assert_eq!(maybe_serialize(&PhpValue::Int(5)), PhpValue::Int(5));
    let object = PhpValue::Object {
        class: "stdClass".into(),
        properties: vec![],
    };
    assert_eq!(maybe_serialize(&object), s(b"O:8:\"stdClass\":0:{}"));
}

#[test]
fn maybe_unserialize_rules() {
    assert_eq!(
        maybe_unserialize(&s(b"a:1:{i:0;i:1;}")),
        PhpValue::Array(vec![(PhpKey::Int(0), PhpValue::Int(1))])
    );
    assert_eq!(maybe_unserialize(&s(b"s:5:\"ab\";")), PhpValue::Bool(false));
    assert_eq!(maybe_unserialize(&s(b"plain")), s(b"plain"));
    assert_eq!(maybe_unserialize(&s(b" i:5; ")), PhpValue::Int(5));
}
