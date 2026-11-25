//! `PostTypeRegistry`: WordPress 7.1.5's `$wp_post_types`, `$_wp_post_type_features` and `$post_type_meta_caps`
//! under register_post_type(), unregister_post_type() and the post type supports functions.

mod support;

use speedyspec_php_serialize::{PhpKey, PhpValue};
use speedyspec_wp_posts::{PostTypeRegistry, check_post_type_name};
use speedyspec_wp_posts_contracts::{PostTypeError, PostTypeName, PostTypeSupports, PostTypes};

fn s(v: &str) -> PhpValue {
    PhpValue::Str(v.as_bytes().to_vec())
}

fn k(v: &str) -> PhpKey {
    PhpKey::Str(v.as_bytes().to_vec())
}

fn list(items: &[&str]) -> PhpValue {
    PhpValue::Array(
        items
            .iter()
            .enumerate()
            .map(|(i, v)| (PhpKey::Int(i as i64), s(v)))
            .collect(),
    )
}

fn register(registry: &mut PostTypeRegistry, name: &str, args: Vec<(PhpKey, PhpValue)>) {
    registry.register(PostTypeName::new(name), args, &support::ports(false));
}

fn names(registry: &PostTypeRegistry) -> Vec<String> {
    registry
        .post_types()
        .iter()
        .map(|t| t.name().as_str().to_owned())
        .collect()
}

fn feature_names(registry: &PostTypeRegistry, post_type: &str) -> Vec<String> {
    registry
        .all_supports(post_type)
        .into_iter()
        .map(|(feature, _)| feature)
        .collect()
}

fn owned(pairs: &[(&str, &'static str)]) -> Vec<(String, &'static str)> {
    pairs.iter().map(|(c, m)| ((*c).to_owned(), *m)).collect()
}

#[test]
fn re_register_keeps_position_and_old_features() {
    let mut registry = PostTypeRegistry::new();
    register(
        &mut registry,
        "book",
        vec![(k("supports"), list(&["title", "comments"]))],
    );
    register(&mut registry, "movie", Vec::new());
    register(
        &mut registry,
        "book",
        vec![
            (k("supports"), list(&["editor"])),
            (k("description"), s("again")),
        ],
    );
    assert_eq!(names(&registry), ["book", "movie"]);
    assert_eq!(
        registry.post_type("book").unwrap().get("description"),
        Some(&s("again"))
    );
    assert_eq!(
        feature_names(&registry, "book"),
        ["title", "comments", "editor", "autosave"]
    );
}

#[test]
fn re_register_accumulates_meta_caps() {
    let mut registry = PostTypeRegistry::new();
    let mapped = |t: &str| {
        vec![
            (k("capability_type"), s(t)),
            (k("map_meta_cap"), PhpValue::Bool(true)),
        ]
    };
    register(&mut registry, "book", mapped("book"));
    register(&mut registry, "book", mapped("tome"));
    assert_eq!(
        registry.meta_caps(),
        owned(&[
            ("edit_book", "edit_post"),
            ("read_book", "read_post"),
            ("delete_book", "delete_post"),
            ("edit_tome", "edit_post"),
            ("read_tome", "read_post"),
            ("delete_tome", "delete_post"),
        ])
    );
    assert_eq!(registry.meta_cap("read_tome"), Some("read_post"));
    assert_eq!(registry.meta_cap("read_scroll"), None);
}

#[test]
fn supports_false_keeps_property_and_adds_nothing() {
    let mut registry = PostTypeRegistry::new();
    let registration = registry.register(
        PostTypeName::new("book"),
        vec![(k("supports"), PhpValue::Bool(false))],
        &support::ports(false),
    );
    assert_eq!(
        registration.post_type.get("supports"),
        Some(&PhpValue::Bool(false))
    );
    assert!(registry.all_supports("book").is_empty());
    assert!(registry.features().is_empty());
}

#[test]
fn supports_empty_adds_defaults_and_keeps_property() {
    for empty in [
        PhpValue::Array(Vec::new()),
        PhpValue::Null,
        s("0"),
        PhpValue::Int(0),
    ] {
        let mut registry = PostTypeRegistry::new();
        let registration = registry.register(
            PostTypeName::new("book"),
            vec![(k("supports"), empty.clone())],
            &support::ports(false),
        );
        assert_eq!(
            feature_names(&registry, "book"),
            ["title", "editor", "autosave"],
            "{empty:?}"
        );
        assert!(
            registration.post_type.get("supports").is_some(),
            "{empty:?}"
        );
    }
}

#[test]
fn supports_list_removes_property() {
    let mut registry = PostTypeRegistry::new();
    let supports = PhpValue::Array(vec![
        (PhpKey::Int(0), s("title")),
        (
            k("editor"),
            PhpValue::Array(vec![(k("notes"), PhpValue::Bool(true))]),
        ),
        (PhpKey::Int(1), s("thumbnail")),
    ]);
    let registration = registry.register(
        PostTypeName::new("book"),
        vec![(k("supports"), supports)],
        &support::ports(false),
    );
    assert_eq!(registration.post_type.get("supports"), None);
    assert_eq!(registry.post_type("book").unwrap().get("supports"), None);
    assert_eq!(
        registry.all_supports("book"),
        vec![
            ("title".to_owned(), PhpValue::Bool(true)),
            (
                "editor".to_owned(),
                PhpValue::Array(vec![(
                    PhpKey::Int(0),
                    PhpValue::Array(vec![(k("notes"), PhpValue::Bool(true))])
                )])
            ),
            ("thumbnail".to_owned(), PhpValue::Bool(true)),
            ("autosave".to_owned(), PhpValue::Bool(true)),
        ]
    );
}

#[test]
fn supports_object_iterates_its_properties() {
    // `foreach` over an object walks its properties, as it does an array's entries.
    let mut registry = PostTypeRegistry::new();
    let supports = PhpValue::Object {
        class: "stdClass".to_owned(),
        properties: vec![
            (k("0"), s("title")),
            (
                k("custom"),
                PhpValue::Array(vec![(k("notes"), PhpValue::Bool(true))]),
            ),
            (k("1"), s("thumbnail")),
        ],
    };
    let registration = registry.register(
        PostTypeName::new("book"),
        vec![(k("supports"), supports)],
        &support::ports(false),
    );
    assert_eq!(registration.post_type.get("supports"), None);
    assert_eq!(
        registry.all_supports("book"),
        vec![
            ("title".to_owned(), PhpValue::Bool(true)),
            (
                "custom".to_owned(),
                PhpValue::Array(vec![(
                    PhpKey::Int(0),
                    PhpValue::Array(vec![(k("notes"), PhpValue::Bool(true))])
                )])
            ),
            ("thumbnail".to_owned(), PhpValue::Bool(true)),
        ]
    );
}

#[test]
fn editor_implies_autosave_unless_present() {
    let mut registry = PostTypeRegistry::new();
    register(
        &mut registry,
        "book",
        vec![(k("supports"), list(&["editor"]))],
    );
    assert_eq!(feature_names(&registry, "book"), ["editor", "autosave"]);

    let mut registry = PostTypeRegistry::new();
    registry.add_support("book", &["autosave"], Vec::new());
    register(
        &mut registry,
        "book",
        vec![(k("supports"), list(&["editor"]))],
    );
    assert_eq!(feature_names(&registry, "book"), ["autosave", "editor"]);

    let mut registry = PostTypeRegistry::new();
    register(
        &mut registry,
        "book",
        vec![(k("supports"), list(&["title"]))],
    );
    assert_eq!(feature_names(&registry, "book"), ["title"]);
    assert!(!registry.supports("book", "autosave"));
}

#[test]
fn remove_last_feature_keeps_the_type_entry() {
    let mut registry = PostTypeRegistry::new();
    registry.add_support("book", &["a", "b"], Vec::new());
    registry.add_support("book", &["c"], vec![s("x"), s("y")]);
    assert!(registry.supports("book", "c"));
    assert_eq!(
        registry.all_supports("book")[2].1,
        PhpValue::Array(vec![(PhpKey::Int(0), s("x")), (PhpKey::Int(1), s("y"))])
    );
    for feature in ["a", "b", "c"] {
        registry.remove_support("book", feature);
    }
    registry.remove_support("missing", "a");
    assert_eq!(registry.features(), vec![("book".to_owned(), Vec::new())]);
}

#[test]
fn unregister_removes_features_and_meta_caps() {
    let mut registry = PostTypeRegistry::new();
    register(&mut registry, "movie", Vec::new());
    register(
        &mut registry,
        "book",
        vec![
            (k("register_meta_box_cb"), s("boxes")),
            (k("capability_type"), s("tome")),
            (k("map_meta_cap"), PhpValue::Bool(true)),
        ],
    );
    let unregistration = registry.unregister("book").unwrap();
    assert_eq!(unregistration.post_type.name().as_str(), "book");
    let removed: Vec<(String, PhpValue, i64)> = unregistration
        .actions
        .into_iter()
        .map(|a| (a.hook, a.callback, a.priority))
        .collect();
    assert_eq!(
        removed,
        vec![
            ("add_meta_boxes_book".to_owned(), s("boxes"), 10),
            ("future_book".to_owned(), s("_future_post_hook"), 5),
        ]
    );
    assert_eq!(names(&registry), ["movie"]);
    assert!(registry.all_supports("book").is_empty());
    assert_eq!(
        registry
            .features()
            .into_iter()
            .map(|(t, _)| t)
            .collect::<Vec<_>>(),
        ["movie"]
    );
    assert_eq!(
        registry.meta_caps(),
        owned(&[
            ("edit_post", "edit_post"),
            ("read_post", "read_post"),
            ("delete_post", "delete_post"),
        ])
    );
}

#[test]
fn unregister_builtin_is_refused() {
    let mut registry = PostTypeRegistry::new();
    register(
        &mut registry,
        "core",
        vec![(k("_builtin"), PhpValue::Bool(true))],
    );
    assert_eq!(
        registry.unregister("core").unwrap_err(),
        PostTypeError::Builtin
    );
    assert_eq!(names(&registry), ["core"]);
    assert!(registry.supports("core", "title"));
}

#[test]
fn unregister_missing_is_invalid() {
    let mut registry = PostTypeRegistry::new();
    assert_eq!(
        registry.unregister("missing").unwrap_err(),
        PostTypeError::Invalid
    );
}

#[test]
fn integer_like_name_is_a_string_key() {
    let mut registry = PostTypeRegistry::new();
    let name = check_post_type_name(b"123").unwrap();
    let registration = registry.register(name, Vec::new(), &support::ports(false));
    assert_eq!(registration.actions.len(), 1);
    assert_eq!(registration.actions[0].hook, "future_123");
    assert!(registry.post_type("123").is_some());
    assert!(registry.supports("123", "editor"));
    assert!(registry.unregister("123").is_ok());
    assert!(registry.post_types().is_empty());
}
