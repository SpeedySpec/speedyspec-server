//! The meta values as WordPress 7.1.5's `meta.php` builds table and column names from a meta type.

use speedyspec_wp_meta_contracts::{MetaKey, MetaType};

fn ty(name: &str) -> MetaType {
    MetaType::new(name).expect("a non-empty type")
}

#[test]
fn meta_type_columns_post() {
    let t = ty("post");
    assert_eq!(t.table_suffix(), "postmeta");
    assert_eq!(t.object_column(), "post_id");
    assert_eq!(t.id_column(), "meta_id");
    assert_eq!(t.cache_group(), "post_meta");
}

#[test]
fn meta_type_columns_user() {
    let t = ty("user");
    assert_eq!(t.table_suffix(), "usermeta");
    assert_eq!(t.object_column(), "user_id");
    assert_eq!(t.id_column(), "umeta_id");
    assert_eq!(t.cache_group(), "user_meta");
}

#[test]
fn meta_type_custom() {
    let t = ty("widget");
    assert_eq!(t.table_suffix(), "widgetmeta");
    assert_eq!(t.object_column(), "widget_id");
    assert_eq!(t.id_column(), "meta_id");
    assert_eq!(t.cache_group(), "widget_meta");
}

#[test]
fn meta_type_object_column_sanitized() {
    let t = ty("Odd Type!");
    assert_eq!(t.object_column(), "oddtype_id");
    assert_eq!(t.table_suffix(), "Odd Type!meta");
    assert_eq!(t.name(), "Odd Type!");
}

#[test]
fn meta_type_empty_is_none() {
    assert!(MetaType::new("").is_none());
}

#[test]
fn meta_key_keeps_bytes() {
    let key = MetaKey::new(vec![0xC2, 0xA0]);
    assert_eq!(key.as_bytes(), &[0xC2, 0xA0]);
    assert_eq!(MetaKey::from("a_b").as_bytes(), b"a_b");
}
