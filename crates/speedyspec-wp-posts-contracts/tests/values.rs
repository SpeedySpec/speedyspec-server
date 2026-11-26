//! The post type and status values and errors, as WordPress 7.1.5 defines them.

use speedyspec_php_serialize::{PhpKey, PhpValue};
use speedyspec_wp_posts_contracts::{
    CommentStatus, MimeType, PingStatus, Post, PostDate, PostDateTime, PostError, PostId,
    PostStatusName, PostStatusObject, PostType, PostTypeError, PostTypeName, Slug,
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
fn setting_the_name_property_keeps_the_field_in_step() {
    let mut pt = post_type();
    pt.set("name", PhpValue::Str(b"tome".to_vec()));
    assert_eq!(pt.name().as_str(), "tome");
    assert_eq!(pt.get("name"), Some(&PhpValue::Str(b"tome".to_vec())));
    // A non-string name leaves the field; removing the property does too.
    pt.set("name", PhpValue::Int(5));
    assert_eq!(pt.name().as_str(), "tome");
    pt.remove("name");
    assert_eq!(pt.name().as_str(), "tome");
    assert!(pt.get("name").is_none());

    let mut st = PostStatusObject::new(PostStatusName::new("draft"), Vec::new());
    st.set("name", PhpValue::Str(b"pending".to_vec()));
    assert_eq!(st.name().as_str(), "pending");
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

#[test]
fn post_default_is_wp_posts_defaults() {
    // schema.php 159-182 and WP_Post's property defaults (7.2-RC2).
    let p = Post::default();
    let zero = PostDate {
        local: PostDateTime::ZERO,
        gmt: PostDateTime::ZERO,
    };
    assert_eq!(p.id, PostId(0));
    assert_eq!(p.author, 0);
    assert_eq!(p.date, zero);
    assert!(p.content.is_empty());
    assert!(p.title.is_empty());
    assert!(p.excerpt.is_empty());
    assert_eq!(p.status, PostStatusName::new("publish"));
    assert_eq!(p.comment_status, CommentStatus::new("open"));
    assert_eq!(p.ping_status, PingStatus::new("open"));
    assert!(p.password.is_empty());
    assert_eq!(p.name, Slug::new(Vec::new()));
    assert!(p.to_ping.is_empty());
    assert!(p.pinged.is_empty());
    assert_eq!(p.modified, zero);
    assert!(p.content_filtered.is_empty());
    assert_eq!(p.parent, PostId(0));
    assert!(p.guid.is_empty());
    assert_eq!(p.menu_order, 0);
    assert_eq!(p.post_type, PostTypeName::new("post"));
    assert_eq!(p.mime_type, MimeType::from(""));
    assert_eq!(p.comment_count, 0);
}

#[test]
fn zero_datetime_is_zero() {
    assert!(PostDateTime::ZERO.is_zero());
    assert_eq!(PostDateTime::ZERO.as_str(), "0000-00-00 00:00:00");
    assert!(PostDateTime::new("0000-00-00 00:00:00").is_zero());
    assert!(!PostDateTime::new("2024-01-01 00:00:00").is_zero());
    assert_eq!(
        PostDateTime::new("2024-01-01 00:00:00").as_str(),
        "2024-01-01 00:00:00"
    );
}

#[test]
fn comment_status_open_is_exact() {
    assert!(CommentStatus::new("open").is_open());
    assert!(!CommentStatus::new("Open").is_open());
    assert!(!CommentStatus::new("closed").is_open());
    assert!(PingStatus::new("open").is_open());
    assert!(!PingStatus::new("Open").is_open());
    assert!(!PingStatus::new("closed").is_open());
}

#[test]
fn slug_keeps_bytes() {
    assert_eq!(Slug::new(vec![0xFF]).as_bytes(), &[0xFF]);
    assert_eq!(Slug::from("hello-world").as_bytes(), b"hello-world");
    assert_eq!(
        MimeType::new(b"image/png".to_vec()).as_bytes(),
        b"image/png"
    );
}

#[test]
fn post_error_displays_the_cause() {
    let e = PostError::Db("table missing".to_string());
    assert_eq!(e.to_string(), "the database failed: table missing");
    let _: &dyn std::error::Error = &e;
}
