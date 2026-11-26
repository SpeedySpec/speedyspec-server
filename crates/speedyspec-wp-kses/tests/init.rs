mod common;

use std::rc::Rc;

use speedyspec_container::Container;
use speedyspec_cqrs::{Command, CommandBus, Query, QueryBus};
use speedyspec_wp_hooks::WpHookApi;
use speedyspec_wp_hooks_contracts::{CallbackId, HasFilterResult, HookName};
use speedyspec_wp_kses::{
    AllowedFrom, AllowedHtmlExplicitValue, AllowedHtmlQuery, AllowedHtmlValue, KsesAttr,
    KsesAttrCheck, KsesHook, KsesInit, KsesInitFilters, KsesPost, KsesQuery, KsesRemoveFilters,
    KsesSplit, KsesSplit2, OneAttr, SafecssFilterAttr, register_kses_handlers,
};

fn has(hooks: &WpHookApi, hook: &str, f: &str) -> bool {
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
            speedyspec_wp_hooks_contracts::HookValue::Str("<p onclick=x>a</p>".into()),
            vec![],
        )
        .unwrap();
    assert_eq!(
        out,
        speedyspec_wp_hooks_contracts::HookValue::Str("<p>a</p>".into())
    );
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
        <AllowedHtmlValue as Query>::KEY,
        <AllowedHtmlExplicitValue as Query>::KEY,
        <AllowedFrom as Query>::KEY,
        <KsesHook as Query>::KEY,
        <KsesSplit as Query>::KEY,
        <KsesSplit2 as Query>::KEY,
        <KsesAttr as Query>::KEY,
        <KsesAttrCheck as Query>::KEY,
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

/// Where the language bridge names a filter (PHP's own `wp_filter_kses`), `kses_init_filters()` registers that one,
/// as WordPress registers the function by name.
#[test]
fn init_filters_prefer_the_bridges_named_functions() {
    use speedyspec_wp_hooks_contracts::{Callback, HookName, HookValue};
    use speedyspec_wp_kses::ValueCallbacks;
    struct Named;
    impl ValueCallbacks for Named {
        fn resolve(&self, _: &HookValue) -> Option<speedyspec_wp_kses_contracts::ValueCallback> {
            None
        }
        fn named_filter(&self, name: &str) -> Option<Callback> {
            let shown = format!("named {name}");
            Some(Callback::rust(name, move |_: &[HookValue]| {
                Ok(HookValue::Str(shown.clone()))
            }))
        }
    }
    let hooks = Rc::new(speedyspec_wp_hooks::WpHookApi::new(
        Rc::new(speedyspec_wp_hooks_memory::MemoryHookStore::new()),
        Rc::new(speedyspec_wp_hooks::IgnoreDeprecations),
    ));
    let api = speedyspec_wp_kses::KsesApi::shared(speedyspec_wp_kses::KsesDeps {
        hooks: hooks.clone(),
        escaper: Rc::new(common::Escaper),
        policy: Rc::new(common::Policy(false)),
        globals: Rc::new(speedyspec_wp_kses::DefaultGlobals::default()),
        uploads: Rc::new(common::Uploads),
        callbacks: Rc::new(Named),
        html_api: Rc::new(speedyspec_wp_kses::NoHtmlApiSanitizer),
        mode: Rc::new(speedyspec_wp_kses::MemoryOperatingMode::default()),
    });
    api.kses_init_filters().unwrap();
    let out = hooks
        .apply_filters(
            &HookName::from("title_save_pre"),
            HookValue::Str("<b>x</b>".into()),
            vec![],
        )
        .unwrap();
    assert_eq!(out, HookValue::Str("named wp_filter_kses".into()));
}
