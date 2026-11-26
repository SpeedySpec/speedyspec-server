//! `$wp_meta_keys` as `MetaKeyRegistry`: insertion order, in-place replacement and `unregister_meta_key()`'s
//! clean-up, following WordPress 7.1.5's `wp-includes/meta.php`.

use speedyspec_php_serialize::{PhpKey, PhpValue};
use speedyspec_wp_meta::MetaKeyRegistry;
use speedyspec_wp_meta_contracts::{MetaKey, MetaKeys, RegisteredMetaKey};

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
    assert_eq!(keys[0], entry("post", "", "color", "b"));
    assert_eq!(keys[1], entry("post", "", "size", "a"));
}

mod defaults {
    use std::cell::Cell;

    use speedyspec_php_serialize::{PhpKey, PhpValue};
    use speedyspec_wp_meta::{MetaKeyRegistry, default_for};
    use speedyspec_wp_meta_contracts::{MetaKey, MetaType, RegisteredMetaKey};

    fn with_args(subtype: &str, key: &str, args: Vec<(PhpKey, PhpValue)>) -> RegisteredMetaKey {
        RegisteredMetaKey {
            object_type: "post".to_owned(),
            subtype: subtype.to_owned(),
            key: MetaKey::from(key),
            args,
        }
    }

    fn default(value: PhpValue) -> Vec<(PhpKey, PhpValue)> {
        vec![(PhpKey::Str(b"default".to_vec()), value)]
    }

    /// Runs `default_for` for `key` on a post, counting the subtype lookups (which answer `page`).
    fn lookup(registry: &MetaKeyRegistry, key: &str, single: bool) -> (Option<PhpValue>, usize) {
        let asked = Cell::new(0);
        let got = default_for(
            registry,
            &MetaType::new("post").unwrap(),
            &MetaKey::from(key),
            single,
            || {
                asked.set(asked.get() + 1);
                Ok::<_, ()>("page".to_owned())
            },
        )
        .unwrap();
        (got, asked.get())
    }

    #[test]
    fn unregistered_key_never_asks_for_the_subtype() {
        let mut registry = MetaKeyRegistry::new();
        registry.insert(with_args("page", "other", default(PhpValue::Int(1))));
        assert_eq!(lookup(&registry, "missing", true), (None, 0));
        assert_eq!(lookup(&MetaKeyRegistry::new(), "missing", true), (None, 0));
    }

    #[test]
    fn key_without_a_default_never_asks_for_the_subtype() {
        let mut registry = MetaKeyRegistry::new();
        registry.insert(with_args("", "k", Vec::new()));
        registry.insert(with_args("page", "k", Vec::new()));
        assert_eq!(lookup(&registry, "k", true), (None, 0));
    }

    #[test]
    fn empty_subtype_default_wins_without_a_lookup() {
        let mut registry = MetaKeyRegistry::new();
        registry.insert(with_args("page", "k", default(PhpValue::Int(2))));
        registry.insert(with_args("", "k", default(PhpValue::Int(1))));
        assert_eq!(lookup(&registry, "k", true), (Some(PhpValue::Int(1)), 0));
    }

    #[test]
    fn subtype_default_is_asked_once() {
        let mut registry = MetaKeyRegistry::new();
        registry.insert(with_args("page", "k", default(PhpValue::Int(2))));
        let list = PhpValue::Array(vec![(PhpKey::Int(0), PhpValue::Int(2))]);
        assert_eq!(lookup(&registry, "k", false), (Some(list), 1));
        let mut other = MetaKeyRegistry::new();
        other.insert(with_args("book", "k", default(PhpValue::Int(2))));
        assert_eq!(lookup(&other, "k", true), (None, 1));
    }

    /// `array_key_exists( 'default', $args )` gathers it and `isset( $defaults[''] )` tests the args array, so a
    /// `null` default is returned as `null` (WordPress 7.2-RC2's `filter_default_metadata()`).
    #[test]
    fn null_default_is_a_default() {
        let mut registry = MetaKeyRegistry::new();
        registry.insert(with_args("", "k", default(PhpValue::Null)));
        assert_eq!(lookup(&registry, "k", true), (Some(PhpValue::Null), 0));
    }
}
