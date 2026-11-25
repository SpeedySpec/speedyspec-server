//! Replays what `compat/posts/record.php` recorded from WordPress 7.1.5: `get_post_type_labels()` over plain
//! objects, with no `post_type_labels_{$post_type}` filter added; `get_post_type_capabilities()` with the meta
//! capabilities it remembers; and `new WP_Post_Type()`'s `set_props()`, under each recorded permalink structure.

mod support;

use speedyspec_php_serialize::semantics::is_empty;
use speedyspec_php_serialize::{PhpKey, PhpValue};
use speedyspec_wp_posts::{
    LabelSource, Untranslated, merge_filtered_labels, meta_capabilities, post_type_capabilities,
    post_type_labels, resolve_post_type,
};
use speedyspec_wp_posts_contracts::PostTypeName;

#[test]
fn labels_match_wordpress() {
    let cases = support::cases();
    let labels = cases["labels"].as_array().unwrap();
    assert_eq!(labels.len(), 12);
    for case in labels {
        let object = support::decode(&case["object"]);
        let PhpValue::Str(name) = support::property(&object, "name").unwrap() else {
            panic!("name is a string");
        };
        let name = String::from_utf8(name.clone()).unwrap();
        let source = LabelSource {
            name: &name,
            label: support::property(&object, "label").filter(|v| !matches!(v, PhpValue::Null)),
            labels: support::property(&object, "labels").unwrap(),
            hierarchical: support::property(&object, "hierarchical").is_some_and(|v| !is_empty(v)),
        };
        let defaults = post_type_labels(&source, &Untranslated);
        // No filter: the filtered labels are the defaults themselves.
        let got = merge_filtered_labels(defaults.clone(), defaults);
        let got = support::normalize_keys(PhpValue::Object {
            class: "stdClass".to_owned(),
            properties: got,
        });
        let want = support::normalize_keys(support::decode(&case["result"]));
        assert_eq!(got, want, "labels/{}", case["name"]);
    }
}

/// A recorded `array( custom => core )` of meta capabilities as pairs.
fn recorded_meta_caps(value: PhpValue) -> Vec<(String, String)> {
    let PhpValue::Array(items) = support::normalize_keys(value) else {
        panic!("meta_caps is an array");
    };
    items
        .into_iter()
        .map(|(key, value)| {
            let (PhpKey::Str(key), PhpValue::Str(value)) = (key, value) else {
                panic!("meta caps are strings");
            };
            (
                String::from_utf8(key).unwrap(),
                String::from_utf8(value).unwrap(),
            )
        })
        .collect()
}

#[test]
fn capabilities_match_wordpress() {
    let cases = support::cases();
    let caps = cases["caps"].as_array().unwrap();
    assert_eq!(caps.len(), 9);
    for case in caps {
        let name = &case["name"];
        let capability_type = support::decode(&case["capability_type"]);
        let PhpValue::Bool(map) = support::decode(&case["map_meta_cap"]) else {
            panic!("map_meta_cap is a bool");
        };
        let capabilities = support::decode(&case["capabilities"]);
        let got = post_type_capabilities(&capability_type, map, &capabilities);
        let object = PhpValue::Object {
            class: "stdClass".to_owned(),
            properties: got.clone(),
        };
        assert_eq!(object, support::decode(&case["result"]), "caps/{name}");

        let meta: Vec<(String, String)> = if map {
            meta_capabilities(&got)
                .into_iter()
                .map(|(custom, core)| (custom, core.to_owned()))
                .collect()
        } else {
            Vec::new()
        };
        assert_eq!(
            meta,
            recorded_meta_caps(support::decode(&case["meta_caps"])),
            "caps/{name} meta caps"
        );
    }
}

#[test]
fn set_props_matches_wordpress() {
    let cases = support::cases();
    let set_props = cases["set_props"].as_array().unwrap();
    assert_eq!(set_props.len(), 21);
    for case in set_props {
        let name = &case["name"];
        let PhpValue::Array(args) = support::decode(&case["args"]) else {
            panic!("args is an array");
        };
        let permalinks = !case["permalinks"].as_str().unwrap().is_empty();
        let resolved = resolve_post_type(
            PostTypeName::new(case["post_type"].as_str().unwrap()),
            args,
            &support::ports(permalinks),
        );
        // The recording is get_object_vars(): the properties as an array.
        let PhpValue::Object { class, properties } = resolved.post_type.to_php() else {
            panic!("to_php() gives an object");
        };
        assert_eq!(class, "WP_Post_Type");
        let got = support::normalize_keys(PhpValue::Array(properties));
        let want = support::normalize_keys(support::decode(&case["object"]));
        assert_eq!(got, want, "set_props/{name}");
    }
}
