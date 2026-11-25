//! The post type and status values and errors, as WordPress 7.1.5 defines them.

use speedyspec_php_serialize::{PhpKey, PhpValue};
use speedyspec_wp_posts_contracts::{
    PostStatusName, PostStatusObject, PostType, PostTypeError, PostTypeName,
};

fn key(s: &str) -> PhpKey {
    PhpKey::Str(s.as_bytes().to_vec())
}

fn post_type() -> PostType {
    PostType::new(
        PostTypeName::new("book"),
        vec![
            (key("name"), PhpValue::Str(b"book".to_vec())),
            (key("public"), PhpValue::Bool(true)),
            (key("hierarchical"), PhpValue::Bool(false)),
        ],
    )
}

#[test]
fn post_type_set_replaces_in_place_and_appends() {
    let mut pt = post_type();
    pt.set("public", PhpValue::Bool(false));
    pt.set("extra", PhpValue::Int(1));
    let keys: Vec<_> = pt.properties().iter().map(|(k, _)| k.clone()).collect();
    assert_eq!(
        keys,
        vec![
            key("name"),
            key("public"),
            key("hierarchical"),
            key("extra")
        ]
    );
    assert!(matches!(pt.get("public"), Some(PhpValue::Bool(false))));
    assert!(matches!(pt.get("extra"), Some(PhpValue::Int(1))));
    assert!(pt.get("missing").is_none());
}

#[test]
fn post_type_remove_drops_the_prop() {
    let mut pt = post_type();
    pt.remove("public");
    assert!(pt.get("public").is_none());
    assert_eq!(pt.properties().len(), 2);
    pt.remove("never_there");
    assert_eq!(pt.properties().len(), 2);
}

#[test]
fn post_type_to_php_is_a_wp_post_type_object() {
    let pt = post_type();
    assert_eq!(pt.name().as_str(), "book");
    match pt.to_php() {
        PhpValue::Object { class, properties } => {
            assert_eq!(class, "WP_Post_Type");
            assert_eq!(properties.len(), 3);
        }
        other => panic!("not an object: {other:?}"),
    }
}

#[test]
fn post_status_to_php_is_stdclass() {
    let mut st = PostStatusObject::new(
        PostStatusName::new("publish"),
        vec![(key("label"), PhpValue::Str(b"Published".to_vec()))],
    );
    st.set("public", PhpValue::Bool(true));
    assert_eq!(st.name().to_string(), "publish");
    match st.to_php() {
        PhpValue::Object { class, properties } => {
            assert_eq!(class, "stdClass");
            assert_eq!(properties.len(), 2);
        }
        other => panic!("not an object: {other:?}"),
    }
    st.remove("label");
    assert!(st.get("label").is_none());
}

#[test]
fn errors_carry_wordpress_codes_and_messages() {
    assert_eq!(PostTypeError::NameLength.code(), "post_type_length_invalid");
    assert_eq!(
        PostTypeError::NameLength.message(),
        "Post type names must be between 1 and 20 characters in length."
    );
    assert_eq!(PostTypeError::Invalid.code(), "invalid_post_type");
    assert_eq!(PostTypeError::Invalid.message(), "Invalid post type.");
    assert_eq!(PostTypeError::Builtin.code(), "invalid_post_type");
    assert_eq!(
        PostTypeError::Builtin.message(),
        "Unregistering a built-in post type is not allowed"
    );
    assert_eq!(PostTypeError::Invalid.to_string(), "Invalid post type.");
    let _: &dyn std::error::Error = &PostTypeError::Builtin;
}
