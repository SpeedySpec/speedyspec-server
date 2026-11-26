use std::cell::Cell;
use std::rc::Rc;

use speedyspec_wp_hooks::{WpHookApi, IgnoreDeprecations};
use speedyspec_wp_hooks_contracts::{Callback, HookName, HookValue, Key};
use speedyspec_wp_hooks_memory::MemoryHookStore;
use speedyspec_wp_options::{
    Autoload, AutoloadPolicy, AutoloadRequest, WordPressAutoloadPolicy, autoload_values,
    register_default_filters,
};

fn s(v: &str) -> HookValue {
    HookValue::Str(v.into())
}

fn hooks() -> Rc<WpHookApi> {
    Rc::new(WpHookApi::new(
        Rc::new(MemoryHookStore::new()),
        Rc::new(IgnoreDeprecations),
    ))
}

fn filter(hooks: &WpHookApi, name: &str, run: impl Fn(&[HookValue]) -> HookValue + 'static) {
    hooks
        .add_filter(
            &HookName::from(name),
            Callback::rust(name, move |args: &[HookValue]| Ok(run(args))),
            &HookValue::Int(10),
            4,
        )
        .unwrap();
}

fn list(items: &[(i64, &str)]) -> HookValue {
    HookValue::Array(items.iter().map(|(k, v)| (Key::Int(*k), s(v))).collect())
}

#[test]
fn explicit_values() {
    let hooks = hooks();
    let fired = Rc::new(Cell::new(false));
    let f = fired.clone();
    filter(&hooks, "wp_default_autoload_value", move |_| {
        f.set(true);
        HookValue::Null
    });
    let p = WordPressAutoloadPolicy::new(hooks);
    let v = s("x");
    assert_eq!(
        p.determine("o", &v, &v, &AutoloadRequest::Bool(true))
            .unwrap(),
        Autoload::On
    );
    assert_eq!(
        p.determine("o", &v, &v, &AutoloadRequest::Bool(false))
            .unwrap(),
        Autoload::Off
    );
    assert_eq!(
        p.determine("o", &v, &v, &AutoloadRequest::Text("yes".into()))
            .unwrap(),
        Autoload::On
    );
    assert_eq!(
        p.determine("o", &v, &v, &AutoloadRequest::Text("no".into()))
            .unwrap(),
        Autoload::Off
    );
    assert!(!fired.get());
}

#[test]
fn default_without_filters_is_auto() {
    let p = WordPressAutoloadPolicy::new(hooks());
    let v = s("x");
    assert_eq!(
        p.determine("o", &v, &v, &AutoloadRequest::Default).unwrap(),
        Autoload::Auto
    );
    assert_eq!(
        p.determine("o", &v, &v, &AutoloadRequest::Text("weird".into()))
            .unwrap(),
        Autoload::Auto
    );
}

#[test]
fn filter_bool_gives_auto_on_off() {
    let hooks = hooks();
    filter(&hooks, "wp_default_autoload_value", |args| {
        assert_eq!(args[0], HookValue::Null);
        assert_eq!(args[1], s("o"));
        HookValue::Bool(args[2] == s("yes"))
    });
    let p = WordPressAutoloadPolicy::new(hooks);
    assert_eq!(
        p.determine("o", &s("yes"), &s("yes"), &AutoloadRequest::Default)
            .unwrap(),
        Autoload::AutoOn
    );
    assert_eq!(
        p.determine("o", &s("no"), &s("no"), &AutoloadRequest::Default)
            .unwrap(),
        Autoload::AutoOff
    );
}

#[test]
fn size_rule() {
    let hooks = hooks();
    register_default_filters(&hooks).unwrap();
    let p = WordPressAutoloadPolicy::new(hooks.clone());
    let at = s(&"x".repeat(150000));
    let over = s(&"x".repeat(150001));
    assert_eq!(
        p.determine("o", &at, &at, &AutoloadRequest::Default)
            .unwrap(),
        Autoload::Auto
    );
    assert_eq!(
        p.determine("o", &over, &over, &AutoloadRequest::Default)
            .unwrap(),
        Autoload::AutoOff
    );
    filter(&hooks, "wp_max_autoloaded_option_size", |args| {
        assert_eq!(args[1], s("small"));
        s("10")
    });
    let eleven = s(&"x".repeat(11));
    assert_eq!(
        p.determine("small", &eleven, &eleven, &AutoloadRequest::Default)
            .unwrap(),
        Autoload::AutoOff
    );
    // `empty()` serialized values have no size.
    assert_eq!(
        p.determine("small", &s("0"), &s("0"), &AutoloadRequest::Default)
            .unwrap(),
        Autoload::Auto
    );
}

#[test]
fn values_to_autoload_default() {
    let p = WordPressAutoloadPolicy::new(hooks());
    let values = p.values_to_autoload().unwrap();
    assert_eq!(
        values,
        list(&[(0, "yes"), (1, "on"), (2, "auto-on"), (3, "auto")])
    );
    assert_eq!(autoload_values(&values), ["yes", "on", "auto-on", "auto"]);
}

#[test]
fn values_to_autoload_only_removes() {
    let hooks = hooks();
    filter(&hooks, "wp_autoload_values_to_autoload", |_| {
        list(&[(0, "on"), (1, "extra")])
    });
    let p = WordPressAutoloadPolicy::new(hooks);
    assert_eq!(p.values_to_autoload().unwrap(), list(&[(0, "on")]));

    let hooks = self::hooks();
    filter(
        &hooks,
        "wp_autoload_values_to_autoload",
        |args| match &args[0] {
            HookValue::Array(items) => HookValue::Array(items[1..].to_vec()),
            other => other.clone(),
        },
    );
    let p = WordPressAutoloadPolicy::new(hooks);
    assert_eq!(
        p.values_to_autoload().unwrap(),
        list(&[(1, "on"), (2, "auto-on"), (3, "auto")])
    );
}
