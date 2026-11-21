use std::rc::Rc;

use serde_json::Value as Json;
use speedyspec_hooks::{Key, Value};
use speedyspec_wp_kses::{
    NoValueCallbacks, add_global_attributes, allow_pdf_objects, allowed_from_value,
    default_post_value, default_tags_value,
};
use speedyspec_wp_kses_contracts::ValueCallback;
use speedyspec_wp_kses_testkit::{PhpOracle, allowed_from_php, bytes, from_bytes};

/// An oracle value as a hook value.
fn hook_value(v: &Json) -> Value {
    match v {
        Json::Null => Value::Null,
        Json::Bool(b) => Value::Bool(*b),
        Json::Number(n) => n
            .as_i64()
            .map(Value::Int)
            .unwrap_or(Value::Float(n.as_f64().unwrap_or(0.0))),
        Json::Object(o) if o.contains_key("array") => Value::Array(
            o["array"]
                .as_array()
                .unwrap()
                .iter()
                .map(|kv| {
                    let key = match &kv[0] {
                        Json::Number(n) => Key::Int(n.as_i64().unwrap()),
                        k => Key::Str(String::from_utf8(from_bytes(k)).unwrap()),
                    };
                    (key, hook_value(&kv[1]))
                })
                .collect(),
        ),
        other => match String::from_utf8(from_bytes(other)) {
            Ok(s) => Value::Str(s),
            Err(e) => Value::Bytes(e.into_bytes()),
        },
    }
}

#[test]
fn defaults_equal_php() {
    let mut php = PhpOracle::start().unwrap();
    let g = php.globals().unwrap();
    let marker: ValueCallback = Rc::new(|_| Ok(true));
    let resolve_json = |_: &[u8]| Some(marker.clone());
    let resolve_value = |_: &Value| Some(marker.clone());
    let ours = allowed_from_value(&default_post_value(), &resolve_value).unwrap();
    assert_eq!(ours, allowed_from_php(&g["allowedposttags"], &resolve_json));
    let ours = allowed_from_value(&default_tags_value(), &resolve_value).unwrap();
    assert_eq!(ours, allowed_from_php(&g["allowedtags"], &resolve_json));
    let _ = NoValueCallbacks;
}

#[test]
fn global_attributes_match_php() {
    let mut php = PhpOracle::start().unwrap();
    let inputs = [
        serde_json::json!(true),
        serde_json::json!(false),
        bytes(b"x"),
        serde_json::json!({"array": []}),
        serde_json::json!({"array": [[{"b64": "aHJlZg=="}, true], [0, {"b64": "eg=="}], [{"b64": "aWQ="}, false]]}),
    ];
    for input in inputs {
        let expected = hook_value(
            &php.call("_wp_add_global_attributes", vec![input.clone()])
                .unwrap()
                .returned,
        );
        assert_eq!(
            add_global_attributes(&hook_value(&input)),
            expected,
            "{input}"
        );
    }
}

#[test]
fn pdf_objects_match_php() {
    let mut php = PhpOracle::start().unwrap();
    let upload = b"http://example.org:8080/wp-content/uploads";
    for url in [
        &b"x.pdf"[..],
        b"x.PDF",
        b"http://example.org:8080/x.pdf",
        b"https://example.org:8080/a/b.pdf",
        b"http://example.org/x.pdf",
        b"http://example.org:8080/x.pdf?y",
        b"http://example.org:8080/x.pdf#y",
        b"javascript:x.pdf",
        b"//example.org:8080/x.pdf",
        b"http://example.org:8080x.pdf",
        b"",
    ] {
        let expected = php
            .call("_wp_kses_allow_pdf_objects", vec![bytes(url)])
            .unwrap()
            .returned
            == Json::Bool(true);
        assert_eq!(
            allow_pdf_objects(url, upload),
            expected,
            "{:?}",
            String::from_utf8_lossy(url)
        );
    }
}
