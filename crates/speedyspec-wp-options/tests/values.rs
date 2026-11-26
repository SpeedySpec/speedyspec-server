use speedyspec_wp_hooks_contracts::{HookValue, Key};
use speedyspec_wp_options::{
    Autoload, AutoloadRequest, OptionName, SerializedValue, php_empty, php_string,
};

fn s(v: &str) -> HookValue {
    HookValue::Str(v.into())
}

#[test]
fn name_trims_and_rejects_empty() {
    assert_eq!(OptionName::new(" foo\n").unwrap().as_str(), "foo");
    for empty in ["", "   ", "0", " 0 ", "\0\x0B\t"] {
        assert!(OptionName::new(empty).is_none(), "{empty:?}");
    }
    assert_eq!(OptionName::new("00").unwrap().as_str(), "00");
    assert_eq!(OptionName::new(" foo ").unwrap().to_string(), "foo");
}

#[test]
fn deprecated_names_map() {
    assert_eq!(
        OptionName::new("blacklist_keys")
            .unwrap()
            .deprecated_replacement(),
        Some("disallowed_keys")
    );
    assert_eq!(
        OptionName::new("comment_whitelist")
            .unwrap()
            .deprecated_replacement(),
        Some("comment_previously_approved")
    );
    assert_eq!(
        OptionName::new("siteurl").unwrap().deprecated_replacement(),
        None
    );
}

#[test]
fn autoload_round_trips() {
    for text in [
        "on", "off", "auto", "auto-on", "auto-off", "yes", "no", "weird",
    ] {
        assert_eq!(Autoload::parse(text).as_str(), text);
    }
    assert_eq!(Autoload::parse("on"), Autoload::On);
    assert_eq!(Autoload::parse("weird"), Autoload::Other("weird".into()));
}

#[test]
fn autoload_request_from_value() {
    assert_eq!(
        AutoloadRequest::from_value(&HookValue::Null),
        AutoloadRequest::Default
    );
    assert_eq!(
        AutoloadRequest::from_value(&HookValue::Bool(false)),
        AutoloadRequest::Bool(false)
    );
    assert_eq!(
        AutoloadRequest::from_value(&HookValue::Int(1)),
        AutoloadRequest::Default
    );
    assert_eq!(
        AutoloadRequest::from_value(&s("no")),
        AutoloadRequest::Text("no".into())
    );
}

#[test]
fn php_string_matches_php() {
    // Each expected text is what PHP 8.5's (string) cast gives (precision = 14).
    for (f, text) in [
        (0.1, "0.1"),
        (1.0, "1"),
        (0.1 + 0.2, "0.3"),
        (1e20, "1.0E+20"),
        (-0.0, "-0"),
        (1e13, "10000000000000"),
        (1e14, "1.0E+14"),
        (123456789012345.0, "1.2345678901234E+14"),
        (0.0001, "0.0001"),
        (0.00001, "1.0E-5"),
        (1.0 / 3.0, "0.33333333333333"),
        (2.0 / 3.0, "0.66666666666667"),
        (-1.5e-7, "-1.5E-7"),
        (99999999999999.5, "1.0E+14"),
        (1.2345678901234568e-5, "1.2345678901235E-5"),
        (f64::INFINITY, "INF"),
        (f64::NEG_INFINITY, "-INF"),
        (f64::NAN, "NAN"),
    ] {
        assert_eq!(
            php_string(&HookValue::Float(f)),
            Some(text.as_bytes().to_vec()),
            "{f}"
        );
    }
    assert_eq!(php_string(&HookValue::Bool(true)), Some(b"1".to_vec()));
    assert_eq!(php_string(&HookValue::Bool(false)), Some(Vec::new()));
    assert_eq!(php_string(&HookValue::Null), Some(Vec::new()));
    assert_eq!(php_string(&HookValue::Int(-7)), Some(b"-7".to_vec()));
    assert_eq!(php_string(&HookValue::Bytes(vec![0xff])), Some(vec![0xff]));
    assert_eq!(
        php_string(&HookValue::Array(vec![(Key::Int(0), s("a"))])),
        None
    );
}

#[test]
fn php_empty_matches_php() {
    for v in [
        s(""),
        s("0"),
        HookValue::Int(0),
        HookValue::Float(0.0),
        HookValue::Bool(false),
        HookValue::Null,
        HookValue::Array(vec![]),
    ] {
        assert!(php_empty(&v), "{v:?}");
    }
    for v in [
        s("0.0"),
        s(" "),
        s("a"),
        HookValue::Int(1),
        HookValue::Array(vec![(Key::Int(0), s(""))]),
    ] {
        assert!(!php_empty(&v), "{v:?}");
    }
}

#[test]
fn serialized_value_formats() {
    assert_eq!(
        SerializedValue::for_insert(&HookValue::Null)
            .unwrap()
            .as_bytes(),
        b""
    );
    assert_eq!(
        SerializedValue::for_insert(&HookValue::Bool(true))
            .unwrap()
            .as_bytes(),
        b"1"
    );
    assert_eq!(SerializedValue::for_update(&HookValue::Null).unwrap(), None);
    assert_eq!(
        SerializedValue::for_update(&HookValue::Int(5))
            .unwrap()
            .map(|v| v.as_bytes().to_vec()),
        Some(b"5".to_vec())
    );
    assert!(SerializedValue::for_insert(&HookValue::Array(vec![])).is_err());
    assert!(
        matches!(SerializedValue::new(vec![0xff]).to_value(), HookValue::Bytes(b) if b == vec![0xff])
    );
    assert!(
        matches!(SerializedValue::new(b"a".to_vec()).to_value(), HookValue::Str(t) if t == "a")
    );
}
