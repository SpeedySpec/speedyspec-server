use serde_json::Value;
use speedyspec_wp_html_syntax::{TagAttribute, start_tag_attributes};
use speedyspec_wp_kses_testkit::{PhpOracle, bytes, from_bytes};

fn attr(name: &str, value: Option<&str>) -> TagAttribute {
    TagAttribute {
        name: name.as_bytes().to_vec(),
        value: value.map(|v| v.as_bytes().to_vec()),
    }
}

#[test]
fn reads_like_the_tag_processor() {
    assert_eq!(
        start_tag_attributes(b"class=\"is-wide\" inert data-lazy='&lt;img&#00062' =/x=/"),
        Some(vec![
            attr("class", Some("is-wide")),
            attr("inert", None),
            attr("data-lazy", Some("<img>")),
            attr("=", None),
            attr("x", Some("/")),
        ])
    );
    assert_eq!(
        start_tag_attributes(b"A=1 a=2"),
        Some(vec![attr("a", Some("1"))])
    );
    assert_eq!(start_tag_attributes(b"a=\"open"), None);
    assert_eq!(start_tag_attributes(b"a>b c"), Some(vec![attr("a", None)]));
    assert_eq!(start_tag_attributes(b""), Some(vec![]));
}

const SUBJECTS: &[&[u8]] = &[
    b"",
    b" ",
    b"/",
    b"a",
    b"a=",
    b"a= ",
    b"a=b",
    b"a = b",
    b"a='b'",
    b"a=\"b\"c",
    b"a='",
    b"a=\"x>\"",
    b"a>b",
    b"A=1 a=2",
    b"=x",
    b"==x",
    b"=/x",
    b"a/b",
    b"a\x00b=c\x00d",
    b"a=\r\nb",
    b"a='\r\n\r'",
    b"on\x0Cclick=x",
    b"href=\"jav&#x09;ascript:x\"",
    b"a=&amp b=&ampx c=&amp;",
    b"x\xC3(=\xC3(",
    b"a b c",
    b"a='1' a='2'",
    b"\"q\"=1",
    b"'q'",
    b"a=b/",
    b"a=b/ c",
    b"< a",
    b"a<b=c",
    b"\ta\n=\x0Cb\r",
];

#[test]
fn matches_php() {
    let mut php = PhpOracle::start().unwrap();
    for s in SUBJECTS {
        let r = php
            .call("html_attributes", vec![bytes(s)])
            .unwrap()
            .returned;
        let expected = match r {
            Value::Null => None,
            v => Some(
                v["array"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|kv| {
                        let pair = kv[1]["array"].as_array().unwrap();
                        let value = &pair[1][1];
                        TagAttribute {
                            name: from_bytes(&pair[0][1]),
                            value: if value == &Value::Bool(true) {
                                None
                            } else {
                                Some(from_bytes(value))
                            },
                        }
                    })
                    .collect::<Vec<_>>(),
            ),
        };
        assert_eq!(
            start_tag_attributes(s),
            expected,
            "{:?}",
            String::from_utf8_lossy(s)
        );
    }
}
