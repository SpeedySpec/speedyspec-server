//! `$wp_meta_keys` as `MetaKeyRegistry`: insertion order, in-place replacement and `unregister_meta_key()`'s
//! clean-up, following WordPress 7.1.5's `wp-includes/meta.php`.

use speedyspec_php_serialize::{PhpKey, PhpValue};
use speedyspec_wp_meta::{MetaKeyRegistry, RegisteredMetaKey};
use speedyspec_wp_meta_contracts::MetaKey;

fn entry(object_type: &str, subtype: &str, key: &str, label: &str) -> RegisteredMetaKey {
    RegisteredMetaKey {
        object_type: object_type.to_owned(),
        subtype: subtype.to_owned(),
        key: MetaKey::from(key),
        args: vec![(
            PhpKey::Str(b"label".to_vec()),
            PhpValue::Str(label.as_bytes().to_vec()),
        )],
    }
}

fn names(registry: &MetaKeyRegistry, object_type: &str, subtype: &str) -> Vec<Vec<u8>> {
    registry
        .keys(object_type, subtype)
        .into_iter()
        .map(|r| r.key.as_bytes().to_vec())
        .collect()
}

#[test]
fn remove_last_key_removes_subtype_and_type() {
    let mut registry = MetaKeyRegistry::new();
    registry.insert(entry("post", "page", "size", "a"));
    let removed = registry
        .remove("post", &MetaKey::from("size"), "page")
        .unwrap();
    assert_eq!(removed, entry("post", "page", "size", "a"));
    assert!(registry.keys("post", "page").is_empty());
    assert!(registry.keys("post", "").is_empty());
    assert_eq!(registry.iter().count(), 0);
    // Registering again starts a fresh type, last in order.
    registry.insert(entry("user", "", "x", "a"));
    registry.insert(entry("post", "", "y", "a"));
    let types: Vec<&str> = registry.iter().map(|r| r.object_type.as_str()).collect();
    assert_eq!(types, ["user", "post"]);
}

#[test]
fn remove_keeps_the_subtypes_that_still_hold_keys() {
    let mut registry = MetaKeyRegistry::new();
    registry.insert(entry("post", "", "color", "a"));
    registry.insert(entry("post", "page", "size", "a"));
    registry.insert(entry("post", "book", "isbn", "a"));
    assert!(
        registry
            .remove("post", &MetaKey::from("size"), "page")
            .is_some()
    );
    assert!(registry.keys("post", "page").is_empty());
    assert_eq!(names(&registry, "post", ""), [b"color".to_vec()]);
    assert_eq!(names(&registry, "post", "book"), [b"isbn".to_vec()]);
    let subtypes: Vec<&str> = registry.iter().map(|r| r.subtype.as_str()).collect();
    assert_eq!(subtypes, ["", "book"]);
}

#[test]
fn keys_of_unknown_type_is_empty() {
    let mut registry = MetaKeyRegistry::new();
    assert!(registry.keys("post", "").is_empty());
    registry.insert(entry("post", "", "color", "a"));
    assert!(registry.keys("comment", "").is_empty());
    assert!(registry.keys("post", "page").is_empty());
    assert!(!registry.exists("comment", &MetaKey::from("color"), ""));
}

#[test]
fn remove_missing_is_none() {
    let mut registry = MetaKeyRegistry::new();
    assert!(
        registry
            .remove("post", &MetaKey::from("color"), "")
            .is_none()
    );
    registry.insert(entry("post", "", "color", "a"));
    assert!(
        registry
            .remove("post", &MetaKey::from("size"), "")
            .is_none()
    );
    assert!(
        registry
            .remove("post", &MetaKey::from("color"), "page")
            .is_none()
    );
    assert!(
        registry
            .remove("user", &MetaKey::from("color"), "")
            .is_none()
    );
    assert!(registry.exists("post", &MetaKey::from("color"), ""));
}

#[test]
fn insert_same_key_replaces_in_place() {
    let mut registry = MetaKeyRegistry::new();
    registry.insert(entry("post", "", "color", "a"));
    registry.insert(entry("post", "", "size", "a"));
    registry.insert(entry("post", "", "color", "b"));
    let keys = registry.keys("post", "");
    assert_eq!(keys.len(), 2);
    assert_eq!(*keys[0], entry("post", "", "color", "b"));
    assert_eq!(*keys[1], entry("post", "", "size", "a"));
}
