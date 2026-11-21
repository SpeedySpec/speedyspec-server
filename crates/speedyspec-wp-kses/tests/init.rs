mod common;

use std::rc::Rc;

use speedyspec_container::Container;
use speedyspec_cqrs::{Command, CommandBus, Query, QueryBus};
use speedyspec_wp_hooks::{CallbackId, HasFilterResult, HookApi, HookName};
use speedyspec_wp_kses::{
    AllowedHtmlQuery, KsesInit, KsesInitFilters, KsesPost, KsesQuery, KsesRemoveFilters, OneAttr,
    SafecssFilterAttr, register_kses_handlers,
};

fn has(hooks: &HookApi, hook: &str, f: &str) -> bool {
    hooks
        .has_filter(
            &HookName::from(hook),
            Some(&CallbackId(Some(f.into()))),
            None,
        )
        .unwrap()
        != HasFilterResult::No
}

#[test]
fn init_filters_registers_kses_on_save_filters() {
    let (hooks, api) = common::setup_shared(false);
    api.kses_init_filters().unwrap();
    assert!(has(&hooks, "title_save_pre", "wp_filter_kses"));
    assert!(has(&hooks, "pre_comment_content", "wp_filter_kses"));
    assert!(!has(&hooks, "pre_comment_content", "wp_filter_post_kses"));
    for hook in [
        "content_save_pre",
        "excerpt_save_pre",
        "content_filtered_save_pre",
    ] {
        assert!(has(&hooks, hook, "wp_filter_post_kses"), "{hook}");
    }
}

#[test]
fn unfiltered_users_get_post_kses_on_comments() {
    let (hooks, api) = common::setup_shared(true);
    api.kses_init_filters().unwrap();
    assert!(has(&hooks, "pre_comment_content", "wp_filter_post_kses"));
}

#[test]
fn remove_filters_removes_every_one() {
    let (hooks, api) = common::setup_shared(false);
    api.kses_init_filters().unwrap();
    api.kses_remove_filters().unwrap();
    for (hook, f) in [
        ("title_save_pre", "wp_filter_kses"),
        ("pre_comment_content", "wp_filter_kses"),
        ("content_save_pre", "wp_filter_post_kses"),
        ("excerpt_save_pre", "wp_filter_post_kses"),
        ("content_filtered_save_pre", "wp_filter_post_kses"),
    ] {
        assert!(!has(&hooks, hook, f), "{hook}");
    }
}

#[test]
fn init_skips_users_with_unfiltered_html() {
    let (hooks, api) = common::setup_shared(true);
    api.kses_init().unwrap();
    assert!(!has(&hooks, "title_save_pre", "wp_filter_kses"));
    let (hooks, api) = common::setup_shared(false);
    api.kses_init().unwrap();
    assert!(has(&hooks, "title_save_pre", "wp_filter_kses"));
}

#[test]
fn registered_filter_sanitizes() {
    let (hooks, api) = common::setup_shared(false);
    api.kses_init_filters().unwrap();
    let out = hooks
        .apply_filters(
            &HookName::from("content_save_pre"),
            speedyspec_wp_hooks::HookValue::Str("<p onclick=x>a</p>".into()),
            vec![],
        )
        .unwrap();
    assert_eq!(out, speedyspec_wp_hooks::HookValue::Str("<p>a</p>".into()));
}

#[test]
fn handlers_are_bound() {
    let (_, api) = common::setup_shared(false);
    let container = Rc::new(Container::new());
    register_kses_handlers(&container, api);
    for key in [
        <KsesInitFilters as Command>::KEY,
        <KsesRemoveFilters as Command>::KEY,
        <KsesInit as Command>::KEY,
    ] {
        assert!(container.has(key), "{key}");
    }
    for key in [
        <KsesQuery as Query>::KEY,
        <KsesPost as Query>::KEY,
        <OneAttr as Query>::KEY,
        <AllowedHtmlQuery as Query>::KEY,
        <SafecssFilterAttr as Query>::KEY,
    ] {
        assert!(container.has(key), "{key}");
    }
    let queries = QueryBus::new(container.clone());
    assert_eq!(
        queries
            .ask(KsesPost {
                data: b"<p onclick=x>a</p>".to_vec()
            })
            .unwrap()
            .unwrap(),
        b"<p>a</p>"
    );
    let commands = CommandBus::new(container);
    commands.dispatch(KsesInitFilters {}).unwrap().unwrap();
}
