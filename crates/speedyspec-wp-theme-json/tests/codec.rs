//! The codec maps any PHP array WordPress would keep to the typed model and back, equal by PHP's `==`.

use proptest::prelude::*;
use speedyspec_hooks::{Key, Value};
use speedyspec_wp_theme_json::{ThemeJsonCodec, php_loose_eq};
use speedyspec_wp_theme_json_contracts::Leaf;
use speedyspec_wp_theme_json_contracts::model::{CssValue, Presets, StyleValue};

fn s(v: &str) -> Value {
    Value::Str(v.to_owned())
}

fn map(entries: Vec<(&str, Value)>) -> Value {
    Value::Array(entries.into_iter().map(|(k, v)| (Key::Str(k.to_owned()), v)).collect())
}

fn list(items: Vec<Value>) -> Value {
    Value::Array(items.into_iter().enumerate().map(|(i, v)| (Key::Int(i as i64), v)).collect())
}

fn round_trip(v: &Value) -> Value {
    ThemeJsonCodec::encode(&ThemeJsonCodec::decode(v))
}

#[test]
fn wrong_typed_leaf_survives() {
    let doc = map(vec![("version", Value::Int(3)), ("settings", map(vec![("appearanceTools", s("true"))]))]);
    let decoded = ThemeJsonCodec::decode(&doc);
    let settings = decoded.settings.as_ref().and_then(Leaf::typed).unwrap();
    assert_eq!(settings.appearance_tools, Some(Leaf::Raw(s("true"))));
    assert!(php_loose_eq(&ThemeJsonCodec::encode(&decoded), &doc));
}

#[test]
fn integer_like_keys_are_lists() {
    let palette = list(vec![map(vec![("slug", s("red")), ("color", s("#f00"))])]);
    let doc = map(vec![("version", Value::Int(3)), ("settings", map(vec![("color", map(vec![("palette", palette)]))]))]);
    let decoded = ThemeJsonCodec::decode(&doc);
    let color = decoded.settings.as_ref().and_then(Leaf::typed).unwrap().color.as_ref().and_then(Leaf::typed).unwrap();
    assert!(matches!(color.palette.as_ref().and_then(Leaf::typed), Some(Presets::Unkeyed(p)) if p.len() == 1));
    assert!(php_loose_eq(&round_trip(&doc), &doc));
}

#[test]
fn presets_keyed_by_origin_decode_by_origin() {
    let palette = map(vec![("theme", list(vec![map(vec![("slug", s("red"))])]))]);
    let doc = map(vec![("settings", map(vec![("color", map(vec![("palette", palette)]))]))]);
    let decoded = ThemeJsonCodec::decode(&doc);
    let color = decoded.settings.as_ref().and_then(Leaf::typed).unwrap().color.as_ref().and_then(Leaf::typed).unwrap();
    assert!(matches!(color.palette.as_ref().and_then(Leaf::typed), Some(Presets::ByOrigin(_))));
    assert!(php_loose_eq(&round_trip(&doc), &doc));
}

#[test]
fn ref_value_decodes() {
    let doc = map(vec![(
        "styles",
        map(vec![("color", map(vec![("text", map(vec![("ref", s("styles.elements.link.color.text"))]))]))]),
    )]);
    let decoded = ThemeJsonCodec::decode(&doc);
    let styles = decoded.styles.as_ref().and_then(Leaf::typed).unwrap();
    let color = styles.properties.color.as_ref().and_then(Leaf::typed).unwrap();
    assert!(matches!(
        color.text.as_ref().and_then(Leaf::typed),
        Some(StyleValue::Css(CssValue::Ref(r))) if r.as_bytes() == b"styles.elements.link.color.text"
    ));
    assert_eq!(round_trip(&doc), doc);
}

#[test]
fn extra_preset_keys_keep_order() {
    let preset = map(vec![("slug", s("a")), ("color", s("#000")), ("name", s("A")), ("x", Value::Int(1)), ("y", Value::Int(2))]);
    let doc = map(vec![("settings", map(vec![("color", map(vec![("palette", list(vec![preset]))]))]))]);
    let Value::Array(top) = round_trip(&doc) else { panic!() };
    let text = format!("{top:?}");
    assert!(text.find("\"x\"").unwrap() < text.find("\"y\"").unwrap());
    assert!(php_loose_eq(&Value::Array(top), &doc));
}

#[test]
fn style_states_and_children_survive() {
    let node = map(vec![
        ("color", map(vec![("text", s("red"))])),
        (":hover", map(vec![("color", map(vec![("text", s("blue"))]))])),
        ("elements", map(vec![("link", map(vec![("color", map(vec![("text", s("green"))]))]))])),
        ("mobile", map(vec![("spacing", map(vec![("padding", map(vec![("top", s("1px"))]))]))])),
    ]);
    let doc = map(vec![("styles", map(vec![("blocks", map(vec![("core/button", node)]))]))]);
    assert!(php_loose_eq(&round_trip(&doc), &doc));
}

#[test]
fn unknown_and_odd_values_survive() {
    let doc = map(vec![
        ("version", s("2")),
        ("bogus", Value::Int(7)),
        ("settings", list(vec![Value::Bool(true)])),
        ("styles", map(vec![("color", s("not an object")), ("typography", map(vec![("fontSize", Value::Float(1.5))]))])),
        ("title", Value::Bytes(vec![0xff, 0xfe])),
    ]);
    assert!(php_loose_eq(&round_trip(&doc), &doc));
}

#[test]
fn loose_eq_follows_php() {
    assert!(php_loose_eq(&map(vec![("a", Value::Int(1)), ("b", Value::Int(2))]), &map(vec![("b", Value::Int(2)), ("a", Value::Int(1))])));
    assert!(php_loose_eq(&s("1"), &Value::Int(1)));
    assert!(!php_loose_eq(&s("abc"), &Value::Int(0)));
    assert!(!php_loose_eq(&map(vec![("a", Value::Int(1))]), &map(vec![("a", Value::Int(1)), ("b", Value::Null)])));
}

#[test]
fn round_trips_a_full_document() {
    let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/twentytwentyfive.sanitized.json")).unwrap();
    let doc = json_to_value(&serde_json::from_str(&text).unwrap());
    assert!(php_loose_eq(&round_trip(&doc), &doc));
}

/// Reads the compat fixture's value encoding (`{"array": [[key, value], …]}`).
fn json_to_value(j: &serde_json::Value) -> Value {
    match j {
        serde_json::Value::Null => Value::Null,
        serde_json::Value::Bool(b) => Value::Bool(*b),
        serde_json::Value::Number(n) => n.as_i64().map(Value::Int).unwrap_or_else(|| Value::Float(n.as_f64().unwrap())),
        serde_json::Value::String(s) => Value::Str(s.clone()),
        serde_json::Value::Object(o) => match o.get("array") {
            Some(serde_json::Value::Array(pairs)) => Value::Array(
                pairs
                    .iter()
                    .map(|p| {
                        let key = match &p[0] {
                            serde_json::Value::Number(n) => Key::Int(n.as_i64().unwrap()),
                            other => Key::Str(other.as_str().unwrap().to_owned()),
                        };
                        (key, json_to_value(&p[1]))
                    })
                    .collect(),
            ),
            _ => Value::Null,
        },
        serde_json::Value::Array(_) => Value::Null,
    }
}

fn leaf_value() -> impl Strategy<Value = Value> {
    let scalar = prop_oneof![
        Just(Value::Null),
        any::<bool>().prop_map(Value::Bool),
        any::<i64>().prop_map(Value::Int),
        "[a-z0-9 #%().-]{0,8}".prop_map(Value::Str),
        proptest::collection::vec(any::<u8>(), 0..4).prop_map(Value::Bytes),
    ];
    scalar.prop_recursive(2, 8, 3, |inner| {
        prop_oneof![
            proptest::collection::vec(inner.clone(), 0..3).prop_map(list),
            proptest::collection::btree_map("[a-z:@]{1,6}", inner, 0..3)
                .prop_map(|kv| Value::Array(kv.into_iter().map(|(k, v)| (Key::Str(k), v)).collect())),
        ]
    })
}

fn schema_doc() -> impl Strategy<Value = Value> {
    let settings_paths = vec![
        vec!["appearanceTools"], vec!["color", "palette"], vec!["color", "custom"], vec!["typography", "fluid"],
        vec!["typography", "fontSizes"], vec!["spacing", "spacingScale"], vec!["spacing", "units"],
        vec!["layout", "contentSize"], vec!["custom"], vec!["blocks", "core/paragraph", "color", "text"],
        vec!["viewport", "mobile"],
    ];
    let styles_paths = vec![
        vec!["color", "text"], vec!["spacing", "padding"], vec!["border", "top"], vec!["shadow"], vec!["css"],
        vec!["elements", "link", "color", "text"], vec!["blocks", "core/group", ":hover", "color", "text"],
        vec!["blocks", "core/group", "variations", "x", "typography", "fontSize"],
    ];
    let paths: Vec<Vec<&'static str>> = settings_paths
        .into_iter()
        .map(|p| [vec!["settings"], p].concat())
        .chain(styles_paths.into_iter().map(|p| [vec!["styles"], p].concat()))
        .collect();
    proptest::collection::vec((proptest::sample::select(paths), leaf_value()), 0..6).prop_map(|entries| {
        let mut doc = map(vec![("version", Value::Int(3))]);
        for (path, value) in entries {
            set_path(&mut doc, &path, value);
        }
        doc
    })
}

fn set_path(doc: &mut Value, path: &[&str], value: Value) {
    let Value::Array(entries) = doc else { return };
    let key = Key::Str(path[0].to_owned());
    if path.len() == 1 {
        if let Some(slot) = entries.iter_mut().find(|(k, _)| *k == key) {
            slot.1 = value;
        } else {
            entries.push((key, value));
        }
        return;
    }
    if !entries.iter().any(|(k, v)| *k == key && matches!(v, Value::Array(_))) {
        entries.retain(|(k, _)| *k != key);
        entries.push((key.clone(), Value::Array(Vec::new())));
    }
    let slot = entries.iter_mut().find(|(k, _)| *k == key).unwrap();
    set_path(&mut slot.1, &path[1..], value);
}

proptest! {
    #[test]
    fn decode_encode_is_identity_on_schema_shaped_values(doc in schema_doc()) {
        prop_assert!(php_loose_eq(&round_trip(&doc), &doc), "{doc:?}\n{:?}", round_trip(&doc));
    }
}
