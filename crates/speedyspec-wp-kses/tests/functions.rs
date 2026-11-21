mod common;

use std::cell::RefCell;
use std::rc::Rc;

use serde_json::Value as Json;
use speedyspec_wp_hooks::{Callback, HookName, HookValue};
use speedyspec_wp_kses::{KsesApi, allow_note_mention_span};
use speedyspec_wp_kses_testkit::{PhpOracle, bytes, from_bytes};

const ATTRS: &[(&[u8], &[u8])] = &[
    (b"href=\"javascript:x\"", b"a"),
    (b"href='http://a'", b"a"),
    (b"title=x", b"a"),
    (b"title=\"a<b\"", b"a"),
    (b"onclick=x", b"a"),
    (b"class = \"a\" ", b"p"),
    (b"  href=x  ", b"a"),
    (b"href=\"x", b"a"),
    (b"", b"a"),
    (b"style=\"color:red;foo:bar\"", b"p"),
    (b"style=\"x:y\"", b"p"),
    (b"data-x=1", b"div"),
    (b"\"", b"a"),
    (b"title=\"\"", b"a"),
    (b"title", b"a"),
    (b"TITLE=X", b"A"),
    (b"href=jav&#x09;ascript:x", b"a"),
    (b"x=y", b"blink"),
    (b"title='it''s'", b"a"),
    (b"title=\"&amp;&lt;\"", b"a"),
];

#[test]
fn one_attr_matches_php() {
    let mut php = PhpOracle::start().unwrap();
    for (attr, element) in ATTRS {
        let (_, api) = common::setup();
        let expected = from_bytes(
            &php.call("wp_kses_one_attr", vec![bytes(attr), bytes(element)])
                .unwrap()
                .returned,
        );
        assert_eq!(
            api.one_attr(attr, element).unwrap(),
            expected,
            "{} on {}",
            common::short(attr),
            common::short(element)
        );
    }
}

const DATA: &[&[u8]] = &[
    b"<p onclick=x>a</p>",
    b"<a href=\\\"x\\\">a</a>",
    b"it\\'s <b>bold</b>",
    b"back\\\\slash",
    b"nul\\0",
    b"<script>x</script>",
    b"<em>x</em><img src=y>",
];

#[test]
fn filter_family_matches_php() {
    let mut php = PhpOracle::start().unwrap();
    for data in DATA {
        for f in [
            "wp_filter_kses",
            "wp_kses_data",
            "wp_filter_post_kses",
            "wp_kses_post",
            "wp_filter_nohtml_kses",
        ] {
            let expected = from_bytes(
                &php.call_in_filter("title_save_pre", f, vec![bytes(data)])
                    .unwrap()
                    .returned,
            );
            let (hooks, api) = common::setup();
            let api = Rc::new(api);
            let got = Rc::new(RefCell::new(Vec::new()));
            let (a, g, name) = (api.clone(), got.clone(), f.to_string());
            hooks
                .add_filter(
                    &HookName::from("title_save_pre"),
                    Callback::rust("probe", move |args: &[HookValue]| {
                        let input = match &args[0] {
                            HookValue::Str(s) => s.as_bytes().to_vec(),
                            HookValue::Bytes(b) => b.clone(),
                            _ => Vec::new(),
                        };
                        *g.borrow_mut() = run(&a, &name, &input);
                        Ok(args[0].clone())
                    }),
                    &HookValue::Int(10),
                    1,
                )
                .unwrap();
            hooks
                .apply_filters(
                    &HookName::from("title_save_pre"),
                    HookValue::Bytes(data.to_vec()),
                    vec![],
                )
                .unwrap();
            assert_eq!(*got.borrow(), expected, "{f} {}", common::short(data));
        }
    }
}

fn run(api: &KsesApi, f: &str, data: &[u8]) -> Vec<u8> {
    match f {
        "wp_filter_kses" => api.filter_kses(data),
        "wp_kses_data" => api.data(data),
        "wp_filter_post_kses" => api.filter_post_kses(data),
        "wp_kses_post" => api.post(data),
        _ => api.filter_nohtml_kses(data),
    }
    .unwrap()
}

#[test]
fn note_mention_span_matches_php() {
    let mut php = PhpOracle::start().unwrap();
    for ctx in ["pre_comment_content", "post"] {
        for allowed in [
            Json::Bool(true),
            serde_json::json!({"array": [[{"b64": "c3Bhbg=="}, true]]}),
            serde_json::json!({"array": []}),
        ] {
            let r = php
                .call(
                    "_wp_kses_allow_note_mention_span",
                    vec![allowed.clone(), Json::String(ctx.into())],
                )
                .unwrap();
            let ours =
                allow_note_mention_span(&common::hook_value(&allowed), &HookValue::Str(ctx.into()));
            assert_eq!(ours, common::hook_value(&r.returned), "{ctx} {allowed}");
        }
    }
}

#[test]
fn version_is_kses_0_2_2() {
    assert_eq!(KsesApi::version(), "0.2.2");
}
