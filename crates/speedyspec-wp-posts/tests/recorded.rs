//! Replays what `compat/posts/record.php` recorded from WordPress 7.1.5: `get_post_type_labels()` over plain
//! objects, with no `post_type_labels_{$post_type}` filter added.

mod support;

use speedyspec_php_serialize::PhpValue;
use speedyspec_php_serialize::semantics::is_empty;
use speedyspec_wp_posts::{LabelSource, Untranslated, merge_filtered_labels, post_type_labels};

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
