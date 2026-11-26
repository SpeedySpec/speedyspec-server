use speedyspec_wp_kses_contracts::{AttrRule, Protocols, RuleValue, UriAttributes};
use speedyspec_wp_kses_lexical::{attr_parse, check_attr_val, hair, hair_parse};
use speedyspec_wp_kses_testkit::{
    PhpOracle, bytes, from_bytes, protocols_value, rule_value, xss_attacks,
};

/// An attribute as name, value, whole and valueless.
type Row = (Vec<u8>, Vec<u8>, Vec<u8>, bool);

fn protocols() -> Protocols {
    Protocols(
        ["http", "https", "mailto"]
            .iter()
            .map(|p| p.as_bytes().into())
            .collect(),
    )
}

fn uris() -> UriAttributes {
    UriAttributes(
        [
            "action",
            "archive",
            "background",
            "cite",
            "classid",
            "codebase",
            "data",
            "formaction",
            "href",
            "icon",
            "longdesc",
            "manifest",
            "poster",
            "profile",
            "src",
            "usemap",
            "xmlns",
        ]
        .iter()
        .map(|p| p.as_bytes().into())
        .collect(),
    )
}

#[test]
fn hair_reads_like_wordpress_docblock() {
    let attrs = hair(
        b"class=\"is-wide\" inert data-lazy='&lt;img&#00062' =/x=/",
        &protocols(),
        &uris(),
    );
    let got: Vec<Row> = attrs
        .iter()
        .map(|a| (a.name.to_vec(), a.value.to_vec(), a.whole.to_vec(), a.vless))
        .collect();
    assert_eq!(
        got,
        vec![
            (
                b"class".to_vec(),
                b"is-wide".to_vec(),
                b"class=\"is-wide\"".to_vec(),
                false
            ),
            (b"inert".to_vec(), b"".to_vec(), b"inert".to_vec(), true),
            (
                b"data-lazy".to_vec(),
                b"&lt;img&gt;".to_vec(),
                b"data-lazy=\"&lt;img&gt;\"".to_vec(),
                false
            ),
            (b"=".to_vec(), b"".to_vec(), b"=".to_vec(), true),
            (b"x".to_vec(), b"/".to_vec(), b"x=\"/\"".to_vec(), false),
        ]
    );
}

#[test]
fn hair_strips_bad_protocols_in_uri_attributes() {
    let attrs = hair(
        b"href=\"javascript:alert(1)\" title=\"javascript:x\"",
        &protocols(),
        &uris(),
    );
    assert_eq!(&*attrs[0].value, b"alert(1)");
    assert_eq!(&*attrs[1].value, b"javascript:x");
}

#[test]
fn attr_parse_keeps_every_byte() {
    assert_eq!(
        attr_parse(b"<a  href=\"x\"  >"),
        Some(vec![&b"<a  "[..], b"href=\"x\"  ", b">"])
    );
    assert_eq!(attr_parse(b"</a>"), None);
    assert_eq!(hair_parse(b""), Some(vec![]));
    assert_eq!(hair_parse(b"a=\"x"), None);
}

#[test]
fn values_are_strict() {
    let rule = AttrRule::Values(vec![RuleValue::Int(1)]);
    assert!(!check_attr_val(b"1", false, &rule).unwrap());
    let rule = AttrRule::Values(vec![RuleValue::Str(b"a".to_vec().into())]);
    assert!(check_attr_val(b"A", false, &rule).unwrap());
}

const HAIR_INPUTS: &[&[u8]] = &[
    b"",
    b" ",
    b"a",
    b"a=b",
    b"A=1 a=2",
    b"href=javascript:x",
    b"href='java&#x09;script:x'",
    b"src=x onerror=y",
    b"a=\"open",
    b"a>b",
    b"title=\"a&amp;b&lt;\"",
    b"title='\"'",
    b"data-x=\"&quot;\"",
    b"x\x00y=\x00",
    b"class=a/",
    b"style=\"color:red\"",
    b"href=\"http://a?b=c&d=e\"",
    b"href=\"HTTP://a\"",
    b"XMLNS=javascript:x",
    b"[[x]]=y",
];

#[test]
fn hair_matches_php() {
    let mut php = PhpOracle::start().unwrap();
    let mut inputs: Vec<Vec<u8>> = HAIR_INPUTS.iter().map(|s| s.to_vec()).collect();
    for attack in xss_attacks() {
        if let Some(head) = speedyspec_wp_kses_lexical::scan::tag_head(&attack) {
            inputs.push(attack[head.attrs].to_vec());
        }
    }
    for input in inputs {
        let r = php
            .call(
                "wp_kses_hair",
                vec![bytes(&input), protocols_value(&protocols())],
            )
            .unwrap()
            .returned;
        let expected: Vec<Row> = r["array"]
            .as_array()
            .unwrap()
            .iter()
            .map(|kv| {
                let f = |k: &str| {
                    kv[1]["array"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .find(|p| from_bytes(&p[0]) == k.as_bytes())
                        .map(|p| from_bytes(&p[1]))
                        .unwrap()
                };
                (f("name"), f("value"), f("whole"), f("vless") == b"y")
            })
            .collect();
        let actual: Vec<Row> = hair(&input, &protocols(), &uris())
            .into_iter()
            .map(|a| {
                (
                    a.name.into_owned(),
                    a.value.into_owned(),
                    a.whole.into_owned(),
                    a.vless,
                )
            })
            .collect();
        assert_eq!(actual, expected, "{:?}", String::from_utf8_lossy(&input));
    }
}

const PARSE_INPUTS: &[&[u8]] = &[
    b"",
    b"<a>",
    b"<a",
    b"< a >",
    b"</a>",
    b"<a href=\"x\">",
    b"<a  href=\"x\"  >",
    b"<a href=x>",
    b"<a href=x/>",
    b"<br />",
    b"<br/>",
    b"<br / >",
    b"<a b c>",
    b"<a b='c'd=\"e\">",
    b"<a b=\"c>",
    b"<a [[x]]>",
    b"<a [x]=y>",
    b"<a b=c\n>",
    b"<a>\n",
    b"<a b>\n",
    b"<a\tb\t=\tc\t>",
    b"<a b=\"\">",
    b"<a b='' c=\"\" d>",
    b"<a-b c>",
    b"<1 x>",
    b"<a b=c'd>",
    b"<a b=\"c\"/>",
    b"<a b=c//>",
    b"<a //>",
    b"<a b=\xC3(>",
    b"<a =b>",
    b"<a b==c>",
];

#[test]
fn attr_parse_matches_php() {
    let mut php = PhpOracle::start().unwrap();
    for input in PARSE_INPUTS {
        for (f, ours) in [
            ("wp_kses_attr_parse", attr_parse(input)),
            ("wp_kses_hair_parse", hair_parse(input)),
        ] {
            let r = php.call(f, vec![bytes(input)]).unwrap().returned;
            let expected = r.get("array").map(|a| {
                a.as_array()
                    .unwrap()
                    .iter()
                    .map(|kv| from_bytes(&kv[1]))
                    .collect::<Vec<_>>()
            });
            let actual = ours.map(|v| v.iter().map(|s| s.to_vec()).collect::<Vec<_>>());
            assert_eq!(actual, expected, "{f} {:?}", String::from_utf8_lossy(input));
        }
    }
}

#[test]
fn check_attr_val_matches_php() {
    let mut php = PhpOracle::start().unwrap();
    let checks: Vec<(&str, serde_json::Value)> = vec![
        ("maxlen", serde_json::json!(3)),
        ("maxlen", bytes(b"3")),
        ("maxlen", bytes(b"abc")),
        ("minlen", serde_json::json!(2)),
        ("maxval", serde_json::json!(10)),
        ("maxval", bytes(b"10")),
        ("minval", serde_json::json!(5)),
        ("valueless", bytes(b"y")),
        ("valueless", bytes(b"N")),
        (
            "values",
            serde_json::json!({"array": [[0, {"b64": "YQ=="}], [1, 1]]}),
        ),
        ("unknown", serde_json::json!(1)),
    ];
    for value in [
        &b""[..],
        b"a",
        b"A",
        b"abcd",
        b"5",
        b" 7 ",
        b"12",
        b"0010",
        b"1234567",
        b"1e1",
        b"x",
    ] {
        for vless in [false, true] {
            for (name, check) in &checks {
                let r = php
                    .call(
                        "wp_kses_check_attr_val",
                        vec![
                            bytes(value),
                            bytes(if vless { b"y" } else { b"n" }),
                            bytes(name.as_bytes()),
                            check.clone(),
                        ],
                    )
                    .unwrap()
                    .returned;
                let rv = rule_value(check);
                let rule = match *name {
                    "maxlen" => AttrRule::MaxLen(rv),
                    "minlen" => AttrRule::MinLen(rv),
                    "maxval" => AttrRule::MaxVal(rv),
                    "minval" => AttrRule::MinVal(rv),
                    "valueless" => AttrRule::Valueless(rv),
                    "values" => match rv {
                        RuleValue::List(l) => AttrRule::Values(l),
                        _ => unreachable!(),
                    },
                    other => AttrRule::Other(other.as_bytes().into(), rv),
                };
                assert_eq!(
                    check_attr_val(value, vless, &rule).unwrap(),
                    r == serde_json::Value::Bool(true),
                    "{name} {check} value {:?} vless {vless}",
                    String::from_utf8_lossy(value)
                );
            }
        }
    }
}
