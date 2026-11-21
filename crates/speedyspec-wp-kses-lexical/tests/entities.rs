use std::borrow::Cow;

use serde_json::Value;
use speedyspec_wp_kses_contracts::EntityContext::{Html, Xml};
use speedyspec_wp_kses_lexical::{
    decode_entities, default_html_entity_names, default_xml_entity_names, normalize_entities,
    valid_unicode,
};
use speedyspec_wp_kses_testkit::{PhpOracle, allowed_from_php, bytes, from_bytes};

fn html(s: &[u8]) -> Vec<u8> {
    normalize_entities(
        s,
        Html,
        default_html_entity_names(),
        default_xml_entity_names(),
    )
    .into_owned()
}

#[test]
fn normalizes_html() {
    assert_eq!(
        html(b"a & b &amp; &#65; &#x41; &nbsp; &bogus;"),
        b"a &amp; b &amp; &#065; &#x41; &nbsp; &amp;bogus;"
    );
    assert_eq!(html(b"&#X41;"), b"&#x41;");
    assert_eq!(html(b"&#00;"), b"&amp;#00;");
    assert_eq!(html(b"&#12345678;"), b"&amp;#12345678;");
    assert_eq!(html(b"&#xD800;"), b"&amp;#xD800;");
    assert_eq!(html(b"&#x0041;"), b"&#x41;");
    assert!(matches!(
        normalize_entities(
            b"no amp",
            Html,
            default_html_entity_names(),
            default_xml_entity_names()
        ),
        Cow::Borrowed(_)
    ));
}

#[test]
fn decodes_numeric() {
    assert_eq!(&*decode_entities(b"&#65;&#x42;"), b"AB");
    assert_eq!(&*decode_entities(b"&#38;#x41;"), b"A");
    assert!(
        valid_unicode(0x9)
            && !valid_unicode(0x8)
            && !valid_unicode(0xFFFE)
            && valid_unicode(0x10FFFF)
    );
}

#[test]
fn default_names_equal_php() {
    let mut php = PhpOracle::start().unwrap();
    let g = php.globals().unwrap();
    for (key, ours) in [
        ("allowedentitynames", default_html_entity_names()),
        ("allowedxmlentitynames", default_xml_entity_names()),
    ] {
        let theirs: std::collections::HashSet<Box<[u8]>> = g[key]["array"]
            .as_array()
            .unwrap()
            .iter()
            .map(|kv| from_bytes(&kv[1]).into())
            .collect();
        assert_eq!(ours.0, theirs, "{key}");
    }
    let _ = allowed_from_php;
}

fn every_entity_input() -> Vec<Vec<u8>> {
    let mut inputs: Vec<Vec<u8>> = [
        &b""[..],
        b"&",
        b"&&",
        b"&amp;",
        b"&amp;amp;",
        b"&#",
        b"&#;",
        b"&#0;",
        b"&#00;",
        b"&#1;",
        b"&#9;",
        b"&#10;",
        b"&#13;",
        b"&#31;",
        b"&#32;",
        b"&#065;",
        b"&#0000065;",
        b"&#00000065;",
        b"&#1234567;",
        b"&#12345678;",
        b"&#55296;",
        b"&#65534;",
        b"&#1114111;",
        b"&#1114112;",
        b"&#x;",
        b"&#x0;",
        b"&#x9;",
        b"&#xa;",
        b"&#XA;",
        b"&#x41",
        b"&#x041;",
        b"&#x00000041;",
        b"&#x10FFFF;",
        b"&#x110000;",
        b"&#xFFFFFF;",
        b"&#x1234567;",
        b"&#xDFFF;",
        b"&#xFFFD;",
        b"&lt;",
        b"&LT;",
        b"&nbsp;",
        b"&nbsp",
        b"&ab;",
        b"&a;",
        b"&abcdefgh;",
        b"&abcdefghi;",
        b"&sup1;",
        b"&sup12;",
        b"&sup123;",
        b"&frac14;",
        b"&apos;",
        b"&quot;",
        b"&hellip;",
        b"&euro;",
        b"&colon;",
        b"&Tab;",
        b"a\xC3(&b",
        b"&#x26;#x41;",
        b"x&#59;y",
        b"&#38;lt;",
    ]
    .iter()
    .map(|s| s.to_vec())
    .collect();
    for name in &default_html_entity_names().0 {
        inputs.push([&b"&"[..], name, b";"].concat());
    }
    inputs
}

#[test]
fn normalize_matches_php() {
    let mut php = PhpOracle::start().unwrap();
    for input in every_entity_input() {
        for (ctx, name) in [(Html, "html"), (Xml, "xml")] {
            let expected = from_bytes(
                &php.call(
                    "wp_kses_normalize_entities",
                    vec![bytes(&input), Value::String(name.into())],
                )
                .unwrap()
                .returned,
            );
            let actual = normalize_entities(
                &input,
                ctx,
                default_html_entity_names(),
                default_xml_entity_names(),
            );
            assert_eq!(
                &*actual,
                &expected[..],
                "{name} {:?}",
                String::from_utf8_lossy(&input)
            );
        }
    }
}

#[test]
fn decode_matches_php() {
    let mut php = PhpOracle::start().unwrap();
    for input in every_entity_input()
        .iter()
        .chain([b"&#255;".to_vec(), b"&#256;".to_vec(), b"&#x1FF;".to_vec()].iter())
    {
        let expected = from_bytes(
            &php.call("wp_kses_decode_entities", vec![bytes(input)])
                .unwrap()
                .returned,
        );
        assert_eq!(
            &*decode_entities(input),
            &expected[..],
            "{:?}",
            String::from_utf8_lossy(input)
        );
    }
}
