//! Each standalone lexical function against its WordPress 7.1.5 twin, over 500 random kses inputs.

use std::cell::RefCell;

use proptest::test_runner::{Config, TestCaseError, TestRunner};
use serde_json::{Value, json};
use speedyspec_wp_kses_contracts::{EntityContext, Protocols, UriAttributes};
use speedyspec_wp_kses_lexical::{
    attr_parse, bad_protocol, decode_entities, default_html_entity_names, default_xml_entity_names,
    hair, hair_parse, html_error, no_null, normalize_entities, stripslashes,
};
use speedyspec_wp_kses_testkit::{PhpOracle, bytes, from_bytes, protocols_value, token_soup};

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

/// Runs `check` against 500 token-soup inputs with one oracle.
fn against_php(check: impl Fn(&mut PhpOracle, &[u8]) -> Result<(), TestCaseError>) {
    let php = RefCell::new(PhpOracle::start().unwrap());
    let mut runner = TestRunner::new(Config {
        cases: 500,
        failure_persistence: None,
        ..Config::default()
    });
    if let Err(e) = runner.run(&token_soup(), |input| check(&mut php.borrow_mut(), &input)) {
        panic!("{e}");
    }
}

fn call(php: &mut PhpOracle, f: &str, args: Vec<Value>) -> Value {
    php.call(f, args).unwrap().returned
}

fn lossy(input: &[u8]) -> String {
    String::from_utf8_lossy(input).into_owned()
}

#[test]
fn entity_functions_match_php() {
    against_php(|php, input| {
        for (ctx, name) in [(EntityContext::Html, "html"), (EntityContext::Xml, "xml")] {
            let expected = from_bytes(&call(
                php,
                "wp_kses_normalize_entities",
                vec![bytes(input), Value::String(name.into())],
            ));
            let actual = normalize_entities(
                input,
                ctx,
                default_html_entity_names(),
                default_xml_entity_names(),
            );
            proptest::prop_assert_eq!(&*actual, &expected[..], "{} {:?}", name, lossy(input));
        }
        let expected = from_bytes(&call(php, "wp_kses_decode_entities", vec![bytes(input)]));
        proptest::prop_assert_eq!(
            &*decode_entities(input),
            &expected[..],
            "{:?}",
            lossy(input)
        );
        Ok(())
    });
}

#[test]
fn text_functions_match_php() {
    against_php(|php, input| {
        let keep = json!({"array": [[{"b64": "c2xhc2hfemVybw=="}, {"b64": "a2VlcA=="}]]});
        let expected = from_bytes(&call(php, "wp_kses_no_null", vec![bytes(input), keep]));
        proptest::prop_assert_eq!(&*no_null(input, true), &expected[..], "{:?}", lossy(input));
        let expected = from_bytes(&call(php, "wp_kses_no_null", vec![bytes(input)]));
        proptest::prop_assert_eq!(&*no_null(input, false), &expected[..], "{:?}", lossy(input));
        let expected = from_bytes(&call(php, "wp_kses_stripslashes", vec![bytes(input)]));
        proptest::prop_assert_eq!(&*stripslashes(input), &expected[..], "{:?}", lossy(input));
        let expected = from_bytes(&call(php, "wp_kses_html_error", vec![bytes(input)]));
        proptest::prop_assert_eq!(html_error(input), &expected[..], "{:?}", lossy(input));
        Ok(())
    });
}

#[test]
fn bad_protocol_matches_php() {
    let p = protocols();
    against_php(|php, input| {
        let expected = from_bytes(&call(
            php,
            "wp_kses_bad_protocol",
            vec![bytes(input), protocols_value(&p)],
        ));
        proptest::prop_assert_eq!(
            &*bad_protocol(input, &p),
            &expected[..],
            "{:?}",
            lossy(input)
        );
        Ok(())
    });
}

#[test]
fn attribute_parsers_match_php() {
    against_php(|php, input| {
        for (f, ours) in [
            ("wp_kses_attr_parse", attr_parse(input)),
            ("wp_kses_hair_parse", hair_parse(input)),
        ] {
            let r = call(php, f, vec![bytes(input)]);
            let expected = r.get("array").map(|a| {
                a.as_array()
                    .unwrap()
                    .iter()
                    .map(|kv| from_bytes(&kv[1]))
                    .collect::<Vec<_>>()
            });
            let actual = ours.map(|v| v.iter().map(|s| s.to_vec()).collect::<Vec<_>>());
            proptest::prop_assert_eq!(actual, expected, "{} {:?}", f, lossy(input));
        }
        Ok(())
    });
}

#[test]
fn hair_matches_php() {
    against_php(|php, input| {
        // PHP's wp_kses_hair reads its URI attributes from wp_kses_uri_attributes(); compare with the same list.
        let uris = php_uris(php);
        let r = call(
            php,
            "wp_kses_hair",
            vec![bytes(input), protocols_value(&protocols())],
        );
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
        let actual: Vec<Row> = hair(input, &protocols(), &uris)
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
        proptest::prop_assert_eq!(actual, expected, "{:?}", lossy(input));
        Ok(())
    });
}

/// wp_kses_uri_attributes() as PHP returns it.
fn php_uris(php: &mut PhpOracle) -> UriAttributes {
    let r = call(php, "wp_kses_uri_attributes", vec![]);
    UriAttributes(
        r["array"]
            .as_array()
            .unwrap()
            .iter()
            .map(|kv| from_bytes(&kv[1]).into())
            .collect(),
    )
}
