mod common;

use serde_json::Value as Json;
use speedyspec_wp_kses_contracts::{AllowedSpec, Context};
use speedyspec_wp_kses_testkit::{PhpOracle, bytes, from_bytes, posts, xss_attacks};

const EDGES: &[&[u8]] = &[
    b"",
    b"plain",
    b"<",
    b">",
    b"a > b",
    b"<p>a</p>",
    b"<P CLASS=x>a</P>",
    b"<p onclick=x>a</p>",
    b"<a href=\"javascript:x\">a</a>",
    b"<!-- c -->",
    b"<!-- <script>x</script> -->",
    b"<!---->",
    b"<!-- a -- b --->",
    b"<!--",
    b"<!--x\ny-->",
    b"</1>",
    b"</1<b>x</b>>",
    b"<!x <b>y</b>>",
    b"<!X>",
    b"<br/>",
    b"<br / >",
    b"<img src=x onerror=y>",
    b"<img src=\"a.png\" alt=\"&quot;\" />",
    b"<a href=\"http://a\" title='\"'>x</a>",
    b"<p style=\"color:red;background:url(javascript:x)\">a</p>",
    b"<p style=\"color: red\">a</p>",
    b"<span style=\"&#99;olor:red\">x</span>",
    b"<div data-x=\"1\" data-Y=\"2\" data-=\"3\">x</div>",
    b"<div data-wp-bind--href=\"x\">x</div>",
    b"<object data=\"x.pdf\"></object>",
    b"<object type=\"application/pdf\" data=\"http://example.org:8080/a.pdf\"></object>",
    b"a\x00b<p\x00>c</p>",
    b"<p>\\\"q\\\"</p>",
    b"&&amp;&#65;&bogus;",
    b"<p title=\"a&b\">x</p>",
    b"<a href=x\x0Bonclick=y>",
    b"<scr<script>ipt>alert(1)</script>",
    b"<a\nhref\n=\nx>",
    b"<a href=\"x\" href=\"y\">",
    b"<p class=\"<x>\">",
    b"<<p>>",
    b"<p<p>>",
    b"</p</p>>",
    b"< p>",
    b"</ p>",
    b"<p/>",
    b"<input type=text>",
    b"<form action=x><input></form>",
    b"<a href='feed:javascript:x'>",
    b"<a rel=\"nofollow\" target=\"_blank\">x</a>",
    b"<abbr title=x>y</abbr>",
    b"<td colspan=2 rowspan=\"3\">",
    b"<ol start=\"5\" reversed>",
    b"<a href=\"a\xC3(b\">x</a>",
    b"\xff\xfe<p>x</p>",
];

fn inputs() -> Vec<Vec<u8>> {
    let mut v: Vec<Vec<u8>> = EDGES.iter().map(|s| s.to_vec()).collect();
    v.extend(xss_attacks());
    v.extend(posts().into_iter().take(60));
    v
}

#[test]
fn matches_php() {
    let mut php = PhpOracle::start().unwrap();
    for input in inputs() {
        for ctx in ["post", "data", "strip", "user_description"] {
            let (_, api) = common::setup();
            let r = php
                .call("wp_kses", vec![bytes(&input), Json::String(ctx.into())])
                .unwrap();
            let ours = api
                .kses(
                    &input,
                    &AllowedSpec::Context(Context::from_bytes(ctx.as_bytes())),
                    None,
                )
                .unwrap();
            assert_eq!(
                String::from_utf8_lossy(&ours),
                String::from_utf8_lossy(&from_bytes(&r.returned)),
                "{ctx}: {}",
                common::short(&input)
            );
        }
    }
}

#[test]
fn hook_counts_match_php() {
    let mut php = PhpOracle::start().unwrap();
    for input in posts()
        .into_iter()
        .take(20)
        .chain(xss_attacks().into_iter().take(40))
    {
        let (hooks, api) = common::setup();
        let r = php
            .call("wp_kses", vec![bytes(&input), Json::String("post".into())])
            .unwrap();
        api.kses(&input, &AllowedSpec::Context(Context::Post), None)
            .unwrap();
        // `attribute_escape` is fired by esc_attr(), behind the AttributeEscaper port the text port owns.
        for (hook, count) in r.filters.iter().filter(|(h, _)| h != "attribute_escape") {
            assert_eq!(
                common::did_filter(&hooks, hook),
                *count as i64,
                "{hook} for {}",
                common::short(&input)
            );
        }
    }
}

#[test]
fn deep_invalid_closers_on_a_small_stack() {
    let mut input = b"</1".repeat(20_000);
    input.push(b'>');
    let mut php = PhpOracle::start().unwrap();
    let expected = from_bytes(
        &php.call("wp_kses", vec![bytes(&input), Json::String("post".into())])
            .unwrap()
            .returned,
    );
    let ours = std::thread::Builder::new()
        .stack_size(256 * 1024)
        .spawn(move || {
            let (_, api) = common::setup();
            api.kses(&input, &AllowedSpec::Context(Context::Post), None)
                .unwrap()
        })
        .unwrap()
        .join()
        .unwrap();
    assert_eq!(ours, expected);
}

#[test]
fn required_attribute_drops_the_tag() {
    use speedyspec_wp_hooks::{HookValue, Key};
    use speedyspec_wp_kses::allowed_from_value;
    use std::rc::Rc;
    let allowed = HookValue::Array(vec![(
        Key::Str("a".into()),
        HookValue::Array(vec![(
            Key::Str("href".into()),
            HookValue::Array(vec![(Key::Str("required".into()), HookValue::Bool(true))]),
        )]),
    )]);
    let allowed = Rc::new(allowed_from_value(&allowed, &|_| None).unwrap());
    let (_, api) = common::setup();
    assert_eq!(
        api.kses(
            b"<a title=x>y</a>",
            &AllowedSpec::Explicit(allowed.clone()),
            None
        )
        .unwrap(),
        b"<a>y</a>"
    );
    assert_eq!(
        api.kses(b"<a href=x>y</a>", &AllowedSpec::Explicit(allowed), None)
            .unwrap(),
        b"<a href=\"x\">y</a>"
    );
}

/// PHP's "repeat until stable" loop never ends on these: each pass adds another `&gt;`. The port drops the token after
/// a fixed number of passes (compat/kses/breaks.json, technical).
#[test]
fn comment_that_never_settles_is_dropped() {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let (_, api) = common::setup();
        let post = AllowedSpec::Context(Context::from_bytes(b"post"));
        let out: Vec<Vec<u8>> = [&b"<!--</<i>"[..], b"a<!--</<i></<i>-->b", b"<!--</<i>-->"]
            .iter()
            .map(|input| api.kses(input, &post, None).unwrap())
            .collect();
        let _ = tx.send(out);
    });
    let out = rx
        .recv_timeout(std::time::Duration::from_secs(10))
        .expect("kses did not finish");
    assert_eq!(out, vec![b"".to_vec(), b"ab".to_vec(), b"".to_vec()]);
}

/// An explicit allowlist that came from another language reaches `pre_kses` as the caller's own value (a PHP array
/// with its value callbacks), not as one rebuilt from [`AllowedHtml`].
#[test]
fn pre_kses_sees_the_callers_own_allowlist() {
    use speedyspec_wp_hooks::{Callback, HookName, HookValue, Key};
    use speedyspec_wp_kses_contracts::Payload;
    let (hooks, api) = common::setup();
    let source = HookValue::Array(vec![(
        Key::Str("b".into()),
        HookValue::Str("callable-stays".into()),
    )]);
    let seen = std::rc::Rc::new(std::cell::RefCell::new(None));
    let keep = seen.clone();
    hooks
        .add_filter(
            &HookName::from("pre_kses"),
            Callback::rust("seen", move |a: &[HookValue]| {
                *keep.borrow_mut() = a.get(1).cloned();
                Ok(a[0].clone())
            }),
            &HookValue::Int(10),
            3,
        )
        .unwrap();
    let html = api.allowed_from(&source).unwrap();
    let spec = AllowedSpec::Foreign {
        html,
        source: Payload(std::rc::Rc::new(source.clone())),
    };
    assert_eq!(
        api.kses(b"<b>x</b><i>y</i>", &spec, None).unwrap(),
        b"<b>x</b>y"
    );
    assert_eq!(*seen.borrow(), Some(source));
}

/// Malformed allowlist rules fail closed where PHP would stop with a TypeError (compat/kses/breaks.json): a
/// `values` rule that is not a list, or a `value_callback` nothing can call, rejects the attribute.
#[test]
fn malformed_rules_reject_the_attribute() {
    use speedyspec_wp_hooks::{HookValue, Key};
    let rule = |name: &str, value: HookValue| {
        HookValue::Array(vec![(
            Key::Str("a".into()),
            HookValue::Array(vec![(
                Key::Str("target".into()),
                HookValue::Array(vec![(Key::Str(name.into()), value)]),
            )]),
        )])
    };
    let (_, api) = common::setup();
    for allowed in [
        rule("values", HookValue::Str("_blank".into())),
        rule("value_callback", HookValue::Str("no_such_function".into())),
    ] {
        let html = api.allowed_from(&allowed).unwrap();
        assert_eq!(
            api.kses(
                b"<a target=\"_blank\">a</a>",
                &AllowedSpec::Explicit(html),
                None
            )
            .unwrap(),
            b"<a>a</a>"
        );
    }
}

/// A `wp_kses_uri_attributes` filter that returns no array leaves WordPress's URI attributes checked, rather than
/// none (PHP stops with a TypeError).
#[test]
fn uri_attributes_filter_returning_no_array_keeps_the_defaults() {
    use speedyspec_wp_hooks::{Callback, HookName, HookValue};
    let (hooks, api) = common::setup();
    hooks
        .add_filter(
            &HookName::from("wp_kses_uri_attributes"),
            Callback::rust("broken", |_: &[HookValue]| {
                Ok(HookValue::Str("href".into()))
            }),
            &HookValue::Int(10),
            1,
        )
        .unwrap();
    assert_eq!(
        api.post(b"<a href=\"javascript:alert(1)\">a</a>").unwrap(),
        b"<a href=\"alert(1)\">a</a>"
    );
}
