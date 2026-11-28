//! The filters phase 2 left for phase 4, against `WpHookApi` over the memory store: `post_type_labels_{$name}`
//! (get_post_type_labels(), post.php 2228-2268), `is_post_type_viewable` and `is_post_status_viewable` with their
//! `true ===` check (2496-2578), and `pre_wp_unique_post_slug` and `wp_unique_post_slug` (5801-5972). None of them
//! has a recording with a callback added, so each case states WordPress's behaviour directly.

mod support;

use std::rc::Rc;

use speedyspec_php_serialize::{PhpKey, PhpValue};
use speedyspec_wp_hooks::WpHookApi;
use speedyspec_wp_hooks_contracts::{Callback, HookError, HookName, HookValue};
use speedyspec_wp_posts::{
    NoBadSlugs, PostStatusRegistry, PostTypePorts, PostTypeRegistry, SlugContext, SlugPorts,
    SlugRequest, Uncached, Untranslated, is_post_status_viewable_filtered,
    is_post_type_viewable_filtered, post_status_name_viewable_filtered,
    post_type_name_viewable_filtered, unique_post_slug_filtered,
};
use speedyspec_wp_posts_contracts::{
    Post, PostId, PostParent, PostRepository, PostRow, PostStatusName, PostType, PostTypeName,
    PostTypes, RewriteContext, Slug, SlugTruncation, TitleSanitizer,
};
use speedyspec_wp_posts_memory::MemoryPostRepository;

use support::Invocations;

fn k(name: &str) -> PhpKey {
    PhpKey::Str(name.as_bytes().to_vec())
}

fn s(text: &str) -> PhpValue {
    PhpValue::Str(text.as_bytes().to_vec())
}

/// A callback at priority 10 that records its arguments and answers `answer(args)`.
fn answer(
    hooks: &WpHookApi,
    log: &Invocations,
    hook: &str,
    answer: impl Fn(&[HookValue]) -> HookValue + 'static,
) {
    let log = log.clone();
    let name = hook.to_owned();
    hooks
        .add_filter(
            &HookName::from(hook),
            Callback::rust(&format!("answer_{hook}"), move |args: &[HookValue]| {
                let reported = args
                    .iter()
                    .enumerate()
                    .map(|(i, a)| {
                        (
                            PhpKey::Int(i as i64),
                            speedyspec_php_serialize::from_hook_value(a).unwrap(),
                        )
                    })
                    .collect();
                log.borrow_mut()
                    .push((name.clone(), PhpValue::Array(reported)));
                Ok::<_, HookError>(answer(args))
            }),
            &HookValue::Int(10),
            99,
        )
        .unwrap();
}

fn fired(log: &Invocations) -> Vec<String> {
    log.borrow().iter().map(|(h, _)| h.clone()).collect()
}

// ---- post_type_labels_{$name} ----

struct NoRewrite;
impl RewriteContext for NoRewrite {
    fn rewrite_applies(&self) -> bool {
        false
    }
}

struct Same;
impl TitleSanitizer for Same {
    fn sanitize_title_with_dashes(&self, title: &[u8]) -> Vec<u8> {
        title.to_vec()
    }
}

fn ports() -> PostTypePorts<'static> {
    PostTypePorts {
        translator: &Untranslated,
        rewrite: &NoRewrite,
        titles: &Same,
    }
}

fn labels_of(post_type: &PostType) -> Vec<(PhpKey, PhpValue)> {
    match post_type.get("labels") {
        Some(PhpValue::Object { class, properties }) => {
            assert_eq!(class, "stdClass");
            properties.clone()
        }
        other => panic!("labels: {other:?}"),
    }
}

fn label(labels: &[(PhpKey, PhpValue)], name: &str) -> PhpValue {
    labels
        .iter()
        .find(|(key, _)| *key == k(name))
        .map(|(_, v)| v.clone())
        .unwrap_or_else(|| panic!("label {name}"))
}

#[test]
fn a_labels_filter_changes_a_label_and_keeps_the_defaults() {
    let hooks = support::hooks();
    let log: Invocations = Rc::default();
    // Answers the labels it was given with `name` replaced and one key added, as an array.
    answer(&hooks, &log, "post_type_labels_book", |args| {
        let PhpValue::Object { properties, .. } =
            speedyspec_php_serialize::from_hook_value(&args[0]).unwrap()
        else {
            panic!("the filter receives the labels object");
        };
        let mut out: Vec<(PhpKey, PhpValue)> = properties
            .into_iter()
            .filter(|(key, _)| *key != k("singular_name"))
            .collect();
        for (key, value) in &mut out {
            if *key == k("name") {
                *value = s("Tomes");
            }
        }
        out.push((k("custom"), s("Custom")));
        speedyspec_php_serialize::to_hook_value(&PhpValue::Array(out))
    });

    let mut registry = PostTypeRegistry::new();
    let unfiltered = registry
        .register(PostTypeName::new("plain"), Vec::new(), &ports())
        .post_type;
    registry
        .register_filtered(PostTypeName::new("book"), Vec::new(), &ports(), &*hooks)
        .unwrap();
    assert_eq!(fired(&log), ["post_type_labels_book"]);

    let book = registry.post_type("book").unwrap();
    let labels = labels_of(&book);
    // The filtered name wins; a key the filter dropped keeps its default; a key it added stays, last.
    assert_eq!(label(&labels, "name"), s("Tomes"));
    assert_eq!(book.get("label"), Some(&s("Tomes")));
    assert_eq!(label(&labels, "singular_name"), s("Post"));
    assert_eq!(labels.last(), Some(&(k("custom"), s("Custom"))));
    assert_eq!(labels.len(), labels_of(&unfiltered).len() + 1);
}

#[test]
fn a_labels_filter_that_answers_a_scalar_adds_key_zero() {
    let hooks = support::hooks();
    let log: Invocations = Rc::default();
    answer(&hooks, &log, "post_type_labels_book", |_| {
        HookValue::Str("x".into())
    });
    let mut registry = PostTypeRegistry::new();
    registry
        .register_filtered(PostTypeName::new("book"), Vec::new(), &ports(), &*hooks)
        .unwrap();
    // (array) 'x' is array( 0 => 'x' ), which array_merge() appends.
    let labels = labels_of(&registry.post_type("book").unwrap());
    assert_eq!(labels.last(), Some(&(PhpKey::Int(0), s("x"))));
    assert_eq!(label(&labels, "name"), s("Posts"));
}

#[test]
fn no_labels_callback_leaves_the_registration_as_register_makes_it() {
    let hooks = support::hooks();
    let mut plain = PostTypeRegistry::new();
    let mut filtered = PostTypeRegistry::new();
    let args = vec![(k("labels"), PhpValue::Array(vec![(k("name"), s("Books"))]))];
    plain.register(PostTypeName::new("book"), args.clone(), &ports());
    filtered
        .register_filtered(PostTypeName::new("book"), args, &ports(), &*hooks)
        .unwrap();
    assert_eq!(
        plain.post_type("book").unwrap().properties(),
        filtered.post_type("book").unwrap().properties()
    );
}

#[test]
fn a_failing_labels_callback_registers_nothing() {
    let hooks = support::hooks();
    hooks
        .add_filter(
            &HookName::from("post_type_labels_book"),
            Callback::rust("boom", |_: &[HookValue]| Err(HookError::message("boom"))),
            &HookValue::Int(10),
            1,
        )
        .unwrap();
    let mut registry = PostTypeRegistry::new();
    let err = registry
        .register_filtered(PostTypeName::new("book"), Vec::new(), &ports(), &*hooks)
        .unwrap_err();
    assert!(err.to_string().contains("boom"));
    assert!(registry.post_type("book").is_none());
}

// ---- is_post_type_viewable and is_post_status_viewable ----

fn post_type(name: &str, props: &[(&str, PhpValue)]) -> PostType {
    let mut properties = vec![(k("name"), s(name))];
    properties.extend(props.iter().map(|(p, v)| (k(p), v.clone())));
    PostType::new(PostTypeName::new(name), properties)
}

struct Types(Vec<PostType>);
impl PostTypes for Types {
    fn post_type(&self, name: &str) -> Option<PostType> {
        self.0.iter().find(|t| t.name().as_str() == name).cloned()
    }
    fn post_types(&self) -> Vec<PostType> {
        self.0.clone()
    }
}

#[test]
fn a_viewable_filter_can_make_a_type_unviewable() {
    let hooks = support::hooks();
    let log: Invocations = Rc::default();
    let book = post_type("book", &[("publicly_queryable", PhpValue::Bool(true))]);
    assert!(is_post_type_viewable_filtered(&*hooks, &book).unwrap());

    answer(&hooks, &log, "is_post_type_viewable", |_| {
        HookValue::Bool(false)
    });
    assert!(!is_post_type_viewable_filtered(&*hooks, &book).unwrap());
    // ( $is_viewable, $post_type ): the computed bool, then the object as WP_Post_Type.
    assert_eq!(
        log.borrow()[0].1,
        PhpValue::Array(vec![
            (PhpKey::Int(0), PhpValue::Bool(true)),
            (
                PhpKey::Int(1),
                PhpValue::Object {
                    class: "WP_Post_Type".to_owned(),
                    properties: book.properties().to_vec(),
                }
            ),
        ])
    );
}

#[test]
fn a_truthy_answer_other_than_true_is_not_viewable() {
    let hooks = support::hooks();
    let log: Invocations = Rc::default();
    answer(&hooks, &log, "is_post_type_viewable", |_| HookValue::Int(1));
    let book = post_type("book", &[("publicly_queryable", PhpValue::Bool(true))]);
    assert!(!is_post_type_viewable_filtered(&*hooks, &book).unwrap());

    let hooks = support::hooks();
    answer(&hooks, &log, "is_post_type_viewable", |_| {
        HookValue::Str("1".into())
    });
    assert!(!is_post_type_viewable_filtered(&*hooks, &book).unwrap());

    // Exactly true makes even a private type viewable.
    let hooks = support::hooks();
    answer(&hooks, &log, "is_post_type_viewable", |_| {
        HookValue::Bool(true)
    });
    let private = post_type("secret", &[("public", PhpValue::Bool(false))]);
    assert!(!speedyspec_wp_posts::is_post_type_viewable(&private));
    assert!(is_post_type_viewable_filtered(&*hooks, &private).unwrap());
}

#[test]
fn a_missing_type_or_status_is_not_viewable_and_fires_nothing() {
    let hooks = support::hooks();
    let log: Invocations = Rc::default();
    answer(&hooks, &log, "is_post_type_viewable", |_| {
        HookValue::Bool(true)
    });
    answer(&hooks, &log, "is_post_status_viewable", |_| {
        HookValue::Bool(true)
    });
    assert!(!post_type_name_viewable_filtered(&*hooks, &Types(Vec::new()), "nope").unwrap());
    assert!(
        !post_status_name_viewable_filtered(&*hooks, &PostStatusRegistry::new(), "nope").unwrap()
    );
    assert!(fired(&log).is_empty());

    let types = Types(vec![post_type("book", &[])]);
    assert!(post_type_name_viewable_filtered(&*hooks, &types, "book").unwrap());
    assert_eq!(fired(&log), ["is_post_type_viewable"]);
}

#[test]
fn status_viewable_filter_runs_only_for_public_statuses() {
    let hooks = support::hooks();
    let log: Invocations = Rc::default();
    let mut statuses = PostStatusRegistry::new();
    statuses.register(b"shown", vec![(k("public"), PhpValue::Bool(false))]);
    statuses.register(b"hidden", vec![(k("internal"), PhpValue::Bool(true))]);

    // An internal status is false before the filter, whatever it would answer.
    answer(&hooks, &log, "is_post_status_viewable", |_| {
        HookValue::Bool(true)
    });
    assert!(!post_status_name_viewable_filtered(&*hooks, &statuses, "hidden").unwrap());
    assert!(fired(&log).is_empty());

    // `shown` is neither internal nor protected but not public, so unfiltered it is not viewable; the filter's true
    // makes it so.
    let shown = speedyspec_wp_posts_contracts::PostStatuses::status(&statuses, "shown").unwrap();
    assert!(!speedyspec_wp_posts::is_post_status_viewable(&shown));
    assert!(is_post_status_viewable_filtered(&*hooks, &shown).unwrap());
    let PhpValue::Array(args) = log.borrow()[0].1.clone() else {
        panic!("args");
    };
    assert_eq!(args[0].1, PhpValue::Bool(false));
    assert!(matches!(&args[1].1, PhpValue::Object { class, .. } if class == "stdClass"));

    let hooks = support::hooks();
    answer(&hooks, &log, "is_post_status_viewable", |_| {
        HookValue::Int(1)
    });
    assert!(!is_post_status_viewable_filtered(&*hooks, &shown).unwrap());
}

// ---- pre_wp_unique_post_slug and wp_unique_post_slug ----

struct Cut;
impl SlugTruncation for Cut {
    fn truncate(&self, slug: &[u8], length: usize) -> Vec<u8> {
        slug[..slug.len().min(length)].to_vec()
    }
}

fn context() -> SlugContext {
    SlugContext {
        feeds: vec![b"feed".to_vec()],
        pagination_base: b"page".to_vec(),
        permalink_structure: b"/%postname%/".to_vec(),
    }
}

fn hierarchical(name: &str, yes: bool) -> PostType {
    post_type(name, &[("hierarchical", PhpValue::Bool(yes))])
}

fn slug_of(
    hooks: &WpHookApi,
    repository: &dyn PostRepository,
    slug: &str,
    status: &str,
    post_type: &str,
) -> PhpValue {
    let types = Types(vec![
        hierarchical("post", false),
        hierarchical("page", true),
        hierarchical("nav_menu_item", true),
    ]);
    let reader = Uncached(repository);
    let ports = SlugPorts {
        repository,
        posts: &reader,
        types: &types,
        truncation: &Cut,
        bad: &NoBadSlugs,
    };
    let request = SlugRequest {
        slug: slug.as_bytes(),
        post_id: PostId(5),
        status,
        post_type,
        parent: PostParent::Int(2),
    };
    unique_post_slug_filtered(hooks, &request, &context(), &ports).unwrap()
}

fn taken(slug: &str) -> MemoryPostRepository {
    let repository = MemoryPostRepository::new();
    repository
        .insert(&PostRow::from_post(&Post {
            name: Slug::new(slug.as_bytes()),
            post_type: PostTypeName::new("post"),
            status: PostStatusName::new("publish"),
            ..Post::default()
        }))
        .unwrap();
    repository
}

#[test]
fn slug_filters_run_in_order_with_their_arguments() {
    let hooks = support::hooks();
    let log: Invocations = Rc::default();
    answer(&hooks, &log, "pre_wp_unique_post_slug", |args| {
        args[0].clone()
    });
    answer(&hooks, &log, "wp_unique_post_slug", |args| {
        let HookValue::Str(slug) = &args[0] else {
            panic!("a string slug");
        };
        HookValue::Str(format!("{slug}!"))
    });

    let out = slug_of(&hooks, &taken("hello"), "hello", "publish", "post");
    assert_eq!(out, s("hello-2!"));
    let i = PhpValue::Int;
    assert_eq!(
        *log.borrow(),
        [
            (
                "pre_wp_unique_post_slug".to_owned(),
                PhpValue::Array(
                    [
                        PhpValue::Null,
                        s("hello"),
                        i(5),
                        s("publish"),
                        s("post"),
                        i(2)
                    ]
                    .into_iter()
                    .enumerate()
                    .map(|(n, v)| (PhpKey::Int(n as i64), v))
                    .collect()
                )
            ),
            (
                "wp_unique_post_slug".to_owned(),
                PhpValue::Array(
                    [
                        s("hello-2"),
                        i(5),
                        s("publish"),
                        s("post"),
                        i(2),
                        s("hello")
                    ]
                    .into_iter()
                    .enumerate()
                    .map(|(n, v)| (PhpKey::Int(n as i64), v))
                    .collect()
                )
            ),
        ]
    );
}

#[test]
fn a_pre_slug_answer_short_circuits() {
    let hooks = support::hooks();
    let log: Invocations = Rc::default();
    // Any answer but null is returned as it is, even a non-string, and the second filter never runs.
    answer(&hooks, &log, "pre_wp_unique_post_slug", |_| {
        HookValue::Int(0)
    });
    answer(&hooks, &log, "wp_unique_post_slug", |args| args[0].clone());
    let out = slug_of(&hooks, &taken("hello"), "hello", "publish", "post");
    assert_eq!(out, PhpValue::Int(0));
    assert_eq!(fired(&log), ["pre_wp_unique_post_slug"]);
}

#[test]
fn unchecked_slugs_fire_no_filter() {
    let hooks = support::hooks();
    let log: Invocations = Rc::default();
    answer(&hooks, &log, "pre_wp_unique_post_slug", |_| {
        HookValue::Str("pre".into())
    });
    answer(&hooks, &log, "wp_unique_post_slug", |_| {
        HookValue::Str("post".into())
    });
    let repository = taken("hello");
    assert_eq!(
        slug_of(&hooks, &repository, "hello", "draft", "post"),
        s("hello")
    );
    assert_eq!(
        slug_of(&hooks, &repository, "hello", "inherit", "revision"),
        s("hello")
    );
    assert!(fired(&log).is_empty());
}

#[test]
fn a_hierarchical_nav_menu_item_skips_the_second_filter() {
    let hooks = support::hooks();
    let log: Invocations = Rc::default();
    answer(&hooks, &log, "pre_wp_unique_post_slug", |_| HookValue::Null);
    answer(&hooks, &log, "wp_unique_post_slug", |_| {
        HookValue::Str("changed".into())
    });
    let out = slug_of(
        &hooks,
        &MemoryPostRepository::new(),
        "item",
        "publish",
        "nav_menu_item",
    );
    assert_eq!(out, s("item"));
    assert_eq!(fired(&log), ["pre_wp_unique_post_slug"]);

    // A page goes through both.
    log.borrow_mut().clear();
    let out = slug_of(
        &hooks,
        &MemoryPostRepository::new(),
        "about",
        "publish",
        "page",
    );
    assert_eq!(out, s("changed"));
    assert_eq!(
        fired(&log),
        ["pre_wp_unique_post_slug", "wp_unique_post_slug"]
    );
}

// ---- a callback's failure reaches the caller whole ----

/// Adds a callback to `hook` that fails with a payload (as a PHP exception object travels), and returns the payload.
fn failing(hooks: &WpHookApi, hook: &str) -> Rc<dyn std::any::Any> {
    let payload: Rc<dyn std::any::Any> = Rc::new(format!("exception from {hook}"));
    let kept = payload.clone();
    hooks
        .add_filter(
            &HookName::from(hook),
            Callback::rust("throws", move |_: &[HookValue]| {
                Err(HookError {
                    message: "thrown".to_owned(),
                    payload: Some(kept.clone()),
                })
            }),
            &HookValue::Int(10),
            99,
        )
        .unwrap();
    payload
}

/// The error is `PostError::Hook` carrying the very payload the callback threw.
fn assert_payload<T: std::fmt::Debug>(
    result: Result<T, speedyspec_wp_posts_contracts::PostError>,
    payload: &Rc<dyn std::any::Any>,
    what: &str,
) {
    match result {
        Err(speedyspec_wp_posts_contracts::PostError::Hook(e)) => {
            assert_eq!(e.message, "thrown", "{what}");
            assert!(
                e.payload.as_ref().is_some_and(|p| Rc::ptr_eq(p, payload)),
                "{what}: the payload is the callback's own"
            );
        }
        other => panic!("{what}: {other:?}"),
    }
}

#[test]
fn sanitize_filter_failures_keep_their_payload() {
    for (hook, field, context) in [
        ("edit_post_title", "post_title", "edit"),
        ("title_save_pre", "post_title", "db"),
        ("post_guid", "guid", "display"),
        ("user_can_richedit", "post_content", "edit"),
        ("attribute_escape", "post_title", "attribute"),
    ] {
        let hooks = support::hooks();
        let payload = failing(&hooks, hook);
        let formatting = support::RustFormatting::new(hooks.clone());
        let ports = speedyspec_wp_posts::SanitizePorts {
            hooks: &*hooks,
            formatting: &formatting,
        };
        let result = speedyspec_wp_posts::sanitize_post_field_word(
            &ports,
            field.as_bytes(),
            s("x"),
            7,
            context,
        );
        assert_payload(result, &payload, hook);

        // The object walk stops at the failing field and reports the same error.
        let result = speedyspec_wp_posts::sanitize_post_value(
            &ports,
            PhpValue::Array(vec![(k(field), s("x"))]),
            context,
        );
        assert_payload(result, &payload, hook);
    }
}

#[test]
fn viewable_filter_failures_keep_their_payload() {
    let hooks = support::hooks();
    let payload = failing(&hooks, "is_post_type_viewable");
    let book = post_type("book", &[]);
    assert_payload(
        is_post_type_viewable_filtered(&*hooks, &book),
        &payload,
        "is_post_type_viewable",
    );

    let hooks = support::hooks();
    let payload = failing(&hooks, "is_post_status_viewable");
    let mut statuses = PostStatusRegistry::new();
    statuses.register(b"shown", vec![(k("public"), PhpValue::Bool(true))]);
    assert_payload(
        post_status_name_viewable_filtered(&*hooks, &statuses, "shown"),
        &payload,
        "is_post_status_viewable",
    );
}

#[test]
fn slug_filter_failures_keep_their_payload() {
    for hook in ["pre_wp_unique_post_slug", "wp_unique_post_slug"] {
        let hooks = support::hooks();
        let payload = failing(&hooks, hook);
        let repository = MemoryPostRepository::new();
        let types = Types(vec![hierarchical("post", false)]);
        let reader = Uncached(&repository);
        let ports = SlugPorts {
            repository: &repository,
            posts: &reader,
            types: &types,
            truncation: &Cut,
            bad: &NoBadSlugs,
        };
        let request = SlugRequest {
            slug: b"hello",
            post_id: PostId(5),
            status: "publish",
            post_type: "post",
            parent: PostParent::NONE,
        };
        assert_payload(
            unique_post_slug_filtered(&*hooks, &request, &context(), &ports),
            &payload,
            hook,
        );
    }
}
