use std::rc::Rc;

use serde_json::json;
use speedyspec_wp_kses_contracts::{AllowedAttrs, AttrRule, AttrRules, ValueCallback};
use speedyspec_wp_kses_testkit::{PhpOracle, allowed_from_php, bytes, from_bytes};

fn oracle() -> PhpOracle {
    PhpOracle::start().expect("the PHP oracle starts")
}

#[test]
fn oracle_runs_wp_kses_post() {
    let mut php = oracle();
    let r = php
        .call("wp_kses_post", vec![bytes(b"<p onclick=\"x\">a</p>")])
        .unwrap();
    assert_eq!(from_bytes(&r.returned), b"<p>a</p>");
}

#[test]
fn invalid_utf8_survives() {
    let mut php = oracle();
    let r = php.call("wp_kses_post", vec![bytes(b"a\xC3(b")]).unwrap();
    assert_eq!(from_bytes(&r.returned), b"a\xC3(b");
}

#[test]
fn filters_are_counted() {
    let mut php = oracle();
    let r = php
        .call(
            "wp_kses",
            vec![bytes(b"<a href=\"x\">a</a><b>b</b>"), json!("post")],
        )
        .unwrap();
    let count = r
        .filters
        .iter()
        .find(|(n, _)| n == "wp_kses_allowed_html")
        .map(|(_, c)| *c);
    assert_eq!(count, Some(4));
}

#[test]
fn globals_dump_has_post_tags() {
    let mut php = oracle();
    let g = php.globals().unwrap();
    let post = allowed_from_php(&g["allowedposttags"], &|_| None);
    match post.get(b"a") {
        Some(AllowedAttrs::Attrs(attrs)) => assert!(attrs.contains_key(&b"href"[..])),
        other => panic!("a: {other:?}"),
    }
}

#[test]
fn odd_shapes_convert() {
    let php_array = json!({"array": [
        ["x", {"array": [
            ["one", 1], ["false", false], ["zero", 0], ["empty", {"b64": ""}], ["null", null], ["yes", {"b64": "eWVz"}], ["arr", {"array": []}],
            ["data", {"array": [["value_callback", {"b64": "X3dwX2tzZXNfYWxsb3dfcGRmX29iamVjdHM="}]]}]
        ]}],
        ["br", true]
    ]});
    let stub: ValueCallback = Rc::new(|_| Ok(true));
    let resolve = |name: &[u8]| (name == b"_wp_kses_allow_pdf_objects").then(|| stub.clone());
    let allowed = allowed_from_php(&php_array, &resolve);
    let Some(AllowedAttrs::Attrs(x)) = allowed.get(b"x") else {
        panic!("x")
    };
    assert_eq!(x[&b"one"[..]], AttrRules::Allowed);
    assert_eq!(x[&b"false"[..]], AttrRules::AllowedFalsy);
    assert_eq!(x[&b"zero"[..]], AttrRules::AllowedFalsy);
    assert_eq!(x[&b"empty"[..]], AttrRules::Denied);
    assert_eq!(x[&b"null"[..]], AttrRules::Denied);
    assert_eq!(x[&b"yes"[..]], AttrRules::Allowed);
    assert_eq!(x[&b"arr"[..]], AttrRules::Rules(vec![]));
    assert!(
        matches!(&x[&b"data"[..]], AttrRules::Rules(r) if matches!(r[..], [AttrRule::ValueCallback(_)]))
    );
    assert_eq!(allowed.get(b"br"), Some(&AllowedAttrs::Empty));
}
