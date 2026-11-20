use speedyspec_php_serialize::{PhpKey, PhpValue, serialize, unserialize};

fn s(b: &[u8]) -> PhpValue {
    PhpValue::Str(b.to_vec())
}

fn ser(v: &PhpValue) -> String {
    String::from_utf8_lossy(&serialize(v)).into_owned()
}

fn round_trip(text: &[u8]) -> PhpValue {
    let v = unserialize(text).unwrap_or_else(|e| panic!("{}: {e}", String::from_utf8_lossy(text)));
    assert_eq!(
        serialize(&v),
        text.to_vec(),
        "{}",
        String::from_utf8_lossy(text)
    );
    v
}

#[test]
fn scalars() {
    assert_eq!(ser(&PhpValue::Null), "N;");
    assert_eq!(ser(&PhpValue::Bool(true)), "b:1;");
    assert_eq!(ser(&PhpValue::Int(-5)), "i:-5;");
    assert_eq!(serialize(&s(b"h\"\xffi")), b"s:4:\"h\"\xffi\";".to_vec());
}

#[test]
fn floats() {
    for (f, text) in [
        (0.1, "d:0.1;"),
        (1.0, "d:1;"),
        (-0.0, "d:-0;"),
        (1e14, "d:100000000000000;"),
        (1e15, "d:1000000000000000;"),
        (9.9e16, "d:99000000000000000;"),
        (1e17, "d:1.0E+17;"),
        (-1.2345678901234568e17, "d:-1.2345678901234568E+17;"),
        (0.0001, "d:0.0001;"),
        (0.00001, "d:1.0E-5;"),
        (1.5e300, "d:1.5E+300;"),
        (f64::INFINITY, "d:INF;"),
        (f64::NEG_INFINITY, "d:-INF;"),
        (f64::NAN, "d:NAN;"),
    ] {
        assert_eq!(ser(&PhpValue::Float(f)), text, "{f}");
    }
    assert_eq!(unserialize(b"d:1.0E+15;").unwrap(), PhpValue::Float(1e15));
    assert!(matches!(unserialize(b"d:NAN;").unwrap(), PhpValue::Float(f) if f.is_nan()));
}

#[test]
fn nested_array() {
    let v = round_trip(b"a:2:{i:0;s:1:\"a\";s:1:\"k\";a:1:{i:5;b:0;}}");
    assert_eq!(
        v,
        PhpValue::Array(vec![
            (PhpKey::Int(0), s(b"a")),
            (
                PhpKey::Str(b"k".to_vec()),
                PhpValue::Array(vec![(PhpKey::Int(5), PhpValue::Bool(false))])
            ),
        ])
    );
}

#[test]
fn objects() {
    for text in [
        &b"O:8:\"stdClass\":1:{s:3:\"foo\";s:3:\"bar\";}"[..],
        b"O:3:\"Foo\":1:{s:8:\"\0Foo\0bar\";i:1;}",
        b"C:3:\"Foo\":5:{hello}",
        b"E:11:\"Suit:Hearts\";",
    ] {
        assert!(
            round_trip(text).contains_object(),
            "{}",
            String::from_utf8_lossy(text)
        );
    }
    assert!(!round_trip(b"a:1:{i:0;i:1;}").contains_object());
}

#[test]
fn references() {
    let v = round_trip(b"a:2:{i:0;i:1;i:1;R:2;}");
    assert_eq!(
        v,
        PhpValue::Array(vec![
            (PhpKey::Int(0), PhpValue::Int(1)),
            (PhpKey::Int(1), PhpValue::Ref(2))
        ])
    );
    let v = round_trip(b"a:2:{i:0;O:8:\"stdClass\":0:{}i:1;r:2;}");
    assert!(v.contains_object());
}

#[test]
fn key_normalisation() {
    assert_eq!(
        unserialize(b"a:1:{s:1:\"5\";i:1;}").unwrap(),
        PhpValue::Array(vec![(PhpKey::Int(5), PhpValue::Int(1))])
    );
    for k in ["05", "-0", " 5", "5 ", "+5", "9223372036854775808"] {
        let text = format!("a:1:{{s:{}:\"{k}\";i:1;}}", k.len());
        assert_eq!(
            unserialize(text.as_bytes()).unwrap(),
            PhpValue::Array(vec![(PhpKey::Str(k.as_bytes().to_vec()), PhpValue::Int(1))]),
            "{k}"
        );
    }
    assert_eq!(
        unserialize(b"a:1:{s:2:\"-5\";i:1;}").unwrap(),
        PhpValue::Array(vec![(PhpKey::Int(-5), PhpValue::Int(1))])
    );
    assert_eq!(
        unserialize(b"O:8:\"stdClass\":1:{i:1;i:2;}").unwrap(),
        PhpValue::Object {
            class: "stdClass".into(),
            // Kept as read: a `__serialize()` class's integer keys must re-serialize as `i:`.
            properties: vec![(PhpKey::Int(1), PhpValue::Int(2))]
        }
    );
}

#[test]
fn errors() {
    match unserialize(b"s:5:\"ab\";") {
        Err(e) => {
            assert_eq!(e.length, 9);
            assert!(e.offset > 0);
            assert_eq!(
                e.to_string(),
                format!("Error at offset {} of 9 bytes", e.offset)
            );
        }
        Ok(v) => panic!("{v:?}"),
    }
    assert!(unserialize(b"s:999999999:\"x\";").is_err());
    assert!(unserialize(b"a:999999999:{}").is_err());
    for bad in [&b"b:2;"[..], b"i:;", b"", b"x:1;", b"a:1:{i:0;}"] {
        assert!(
            unserialize(bad).is_err(),
            "{}",
            String::from_utf8_lossy(bad)
        );
    }
    let deep = format!("{}i:1;{}", "a:1:{i:0;".repeat(4097), "}".repeat(4097));
    assert!(unserialize(deep.as_bytes()).is_err());
    let ok = format!("{}i:1;{}", "a:1:{i:0;".repeat(4000), "}".repeat(4000));
    assert!(unserialize(ok.as_bytes()).is_ok());
}

#[test]
fn trailing_bytes_ignored() {
    assert_eq!(unserialize(b"i:1;junk").unwrap(), PhpValue::Int(1));
}

#[test]
fn object_integer_keys_round_trip() {
    // `__serialize()` output (ArrayObject's) keeps integer keys.
    round_trip(b"O:11:\"ArrayObject\":2:{i:0;i:0;i:1;a:0:{}}");
}
