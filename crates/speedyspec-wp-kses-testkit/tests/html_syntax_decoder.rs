use serde_json::Value;
use speedyspec_wp_html_syntax::{
    DecodeContext, decode, decode_attribute, named_reference, read_character_reference,
};
use speedyspec_wp_kses_testkit::{PhpOracle, bytes, from_bytes};

#[test]
fn named_references_take_the_longest() {
    assert_eq!(named_reference(b"amp;x", 0), Some((&b"&"[..], 4)));
    assert_eq!(named_reference(b"ampx", 0), Some((&b"&"[..], 3)));
    assert_eq!(named_reference(b"notin;", 0), Some(("∉".as_bytes(), 6)));
    assert_eq!(named_reference(b"zzz", 0), None);
}

#[test]
fn numeric_references() {
    assert_eq!(
        read_character_reference(DecodeContext::Data, b"&#65;", 0),
        Some((b"A".to_vec(), 5))
    );
    assert_eq!(
        read_character_reference(DecodeContext::Data, b"&#x80;", 0),
        Some(("€".as_bytes().to_vec(), 6))
    );
    assert_eq!(
        read_character_reference(DecodeContext::Data, b"&#0;", 0),
        Some(("\u{FFFD}".as_bytes().to_vec(), 4))
    );
    assert_eq!(
        read_character_reference(DecodeContext::Data, b"&#xD800;", 0),
        Some(("\u{FFFD}".as_bytes().to_vec(), 8))
    );
    assert_eq!(
        read_character_reference(DecodeContext::Data, b"&#;", 0),
        None
    );
}

#[test]
fn attribute_context_is_stricter_without_semicolon() {
    assert_eq!(&*decode_attribute(b"a&ampb"), b"a&ampb");
    assert_eq!(&*decode(DecodeContext::Data, b"a&ampb"), b"a&b");
    assert_eq!(&*decode_attribute(b"a&amp b"), b"a& b");
}

const SUBJECTS: &[&[u8]] = &[
    b"",
    b"&",
    b"&&",
    b"&amp",
    b"&amp;",
    b"&ampx",
    b"&amp=",
    b"&amp1",
    b"&lt;&gt;",
    b"&LT",
    b"&#",
    b"&#x",
    b"&#;",
    b"&#0",
    b"&#00;",
    b"&#65",
    b"&#065;",
    b"&#x41;",
    b"&#X41",
    b"&#1114111;",
    b"&#1114112;",
    b"&#99999999;",
    b"&#x110000;",
    b"&#xFFFFFFF;",
    b"&#128;",
    b"&#x9F;",
    b"&#x81;",
    b"&#xD800;",
    b"&#xDFFF;",
    b"&#13;",
    b"&notin;",
    b"&notit;",
    b"&noti",
    b"a\xC3(&amp;b",
    b"&nbsp;&copy&reg;",
    b"&aacute&Aacute;",
    b"&CounterClockwiseContourIntegral;",
    b"&javascript&colon;x",
    b"&Tab;&NewLine;",
    b"x&#x0000000041;y",
    b"&#0000000065;",
];

#[test]
fn decode_matches_php() {
    let mut php = PhpOracle::start().unwrap();
    for s in SUBJECTS {
        for (ctx, name) in [
            (DecodeContext::Attribute, "attribute"),
            (DecodeContext::Data, "data"),
        ] {
            let expected = from_bytes(
                &php.call("html_decode", vec![Value::String(name.into()), bytes(s)])
                    .unwrap()
                    .returned,
            );
            assert_eq!(
                &*decode(ctx, s),
                &expected[..],
                "{name} {:?}",
                String::from_utf8_lossy(s)
            );
        }
    }
}

#[test]
fn table_matches_php() {
    let mut php = PhpOracle::start().unwrap();
    let all = php.call("html_named_references", vec![]).unwrap().returned;
    let pairs = all["array"].as_array().unwrap();
    assert_eq!(
        pairs.len(),
        speedyspec_wp_html_syntax::named_reference_count()
    );
    for pair in pairs {
        let (name, value) = (from_bytes(&pair[0]), from_bytes(&pair[1]));
        let mut text = name.clone();
        text.push(b'\0');
        let (found, len) = named_reference(&text, 0).unwrap();
        assert_eq!(
            (found, len),
            (&value[..], name.len()),
            "{:?}",
            String::from_utf8_lossy(&name)
        );
    }
}
