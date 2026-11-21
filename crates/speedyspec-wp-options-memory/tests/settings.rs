use std::cell::RefCell;
use std::rc::Rc;

use speedyspec_wp_hooks::{
    Callback, CallbackId, HasFilterResult, HookApi, HookName, HookValue, IgnoreDeprecations, Key,
};
use speedyspec_wp_hooks_memory::MemoryHookStore;
use speedyspec_wp_object_cache::{CurrentObjectCache, ObjectCachePorts};
use speedyspec_wp_options::{
    ArgsParsing, Deprecations, FILTER_DEFAULT_OPTION, OptionsApi, OptionsDeps, OptionsError,
    SettingArgsInput, SettingsRegistry, SettingsStore,
};
use speedyspec_wp_options_memory::{MemoryOptionRepository, MemorySettingsStore};

fn s(v: &str) -> HookValue {
    HookValue::Str(v.into())
}

fn array(items: &[(&str, HookValue)]) -> HookValue {
    HookValue::Array(
        items
            .iter()
            .map(|(k, v)| (Key::Str(k.to_string()), v.clone()))
            .collect(),
    )
}

#[derive(Default)]
struct Notices {
    deprecated: RefCell<Vec<(String, String, String)>>,
    wrong: RefCell<Vec<(String, String, String)>>,
}
impl Deprecations for Notices {
    fn deprecated_argument(
        &self,
        function: &str,
        version: &str,
        message: &str,
    ) -> Result<(), OptionsError> {
        self.deprecated
            .borrow_mut()
            .push((function.into(), version.into(), message.into()));
        Ok(())
    }
    fn doing_it_wrong(
        &self,
        function: &str,
        message: &str,
        version: &str,
    ) -> Result<(), OptionsError> {
        self.wrong
            .borrow_mut()
            .push((function.into(), message.into(), version.into()));
        Ok(())
    }
}

struct Rig {
    registry: Rc<SettingsRegistry>,
    store: Rc<MemorySettingsStore>,
    hooks: Rc<HookApi>,
    notices: Rc<Notices>,
}

fn rig() -> Rig {
    let hooks = Rc::new(HookApi::new(
        Rc::new(MemoryHookStore::new()),
        Rc::new(IgnoreDeprecations),
    ));
    let store = Rc::new(MemorySettingsStore::new());
    let notices = Rc::new(Notices::default());
    let registry = Rc::new(SettingsRegistry::new(
        store.clone(),
        hooks.clone(),
        notices.clone(),
    ));
    Rig {
        registry,
        store,
        hooks,
        notices,
    }
}

fn keys(v: &HookValue) -> Vec<String> {
    match v {
        HookValue::Array(items) => items
            .iter()
            .map(|(k, _)| match k {
                Key::Str(s) => s.clone(),
                Key::Int(i) => i.to_string(),
            })
            .collect(),
        other => panic!("{other:?}"),
    }
}

fn field(v: &HookValue, name: &str) -> Option<HookValue> {
    match v {
        HookValue::Array(items) => items
            .iter()
            .find(|(k, _)| *k == Key::Str(name.into()))
            .map(|(_, v)| v.clone()),
        _ => None,
    }
}

#[test]
fn defaults_merged_in_order() {
    let r = rig();
    r.registry
        .register_setting(
            "g",
            "n",
            SettingArgsInput::Array(array(&[("default", s("x"))])),
        )
        .unwrap();
    let args = r.registry.registered_settings()["n"].clone();
    assert_eq!(
        keys(&args),
        [
            "type",
            "group",
            "label",
            "description",
            "sanitize_callback",
            "show_in_rest",
            "default"
        ]
    );
    assert_eq!(field(&args, "type"), Some(s("string")));
    assert_eq!(field(&args, "group"), Some(s("g")));
    assert_eq!(field(&args, "sanitize_callback"), Some(HookValue::Null));
    assert_eq!(field(&args, "show_in_rest"), Some(HookValue::Bool(false)));
}

#[test]
fn callable_becomes_sanitize_callback() {
    let r = rig();
    let cb = Callback::rust("smarten", |args: &[HookValue]| match &args[0] {
        HookValue::Str(v) => Ok(HookValue::Str(v.to_uppercase())),
        other => Ok(other.clone()),
    });
    r.registry
        .register_setting("g", "n", SettingArgsInput::Callable(cb))
        .unwrap();
    let out = r
        .hooks
        .apply_filters(&HookName::from("sanitize_option_n"), s("smart"), vec![])
        .unwrap();
    assert_eq!(out, s("SMART"));
}

#[test]
fn default_filter_registered() {
    let r = rig();
    r.registry
        .register_setting(
            "g",
            "n",
            SettingArgsInput::Array(array(&[("default", s("x"))])),
        )
        .unwrap();
    let id = CallbackId(Some(FILTER_DEFAULT_OPTION.into()));
    let found = r
        .hooks
        .has_filter(&HookName::from("default_option_n"), Some(&id), None)
        .unwrap();
    assert!(matches!(found, HasFilterResult::At(_)), "{found:?}");
    assert_eq!(
        format!("{found:?}"),
        format!(
            "{:?}",
            HasFilterResult::At(speedyspec_wp_hooks::ArrayKey::Int(10))
        )
    );
}

#[test]
fn filter_default_option_rules() {
    let r = rig();
    r.registry
        .register_setting(
            "g",
            "n",
            SettingArgsInput::Array(array(&[("default", s("x"))])),
        )
        .unwrap();
    assert_eq!(
        r.registry.filter_default_option(s("passed"), "n", true),
        s("passed")
    );
    assert_eq!(
        r.registry
            .filter_default_option(s("kept"), "unknown", false),
        s("kept")
    );
    assert_eq!(
        r.registry
            .filter_default_option(HookValue::Bool(false), "n", false),
        s("x")
    );
}

#[test]
fn get_option_uses_registered_default() {
    let r = rig();
    let current = Rc::new(CurrentObjectCache::new(ObjectCachePorts::rust(), None));
    let api = OptionsApi::new(OptionsDeps::rust(
        r.hooks.clone(),
        current,
        Rc::new(MemoryOptionRepository::new()),
    ));
    r.registry
        .register_setting(
            "g",
            "n",
            SettingArgsInput::Array(array(&[("default", s("x"))])),
        )
        .unwrap();
    assert_eq!(api.get_option("n", None).unwrap(), s("x"));
    assert_eq!(api.get_option("n", Some(s("y"))).unwrap(), s("y"));
}

#[test]
fn args_filter_sees_raw_args() {
    let r = rig();
    let seen: Rc<RefCell<Vec<HookValue>>> = Rc::default();
    let log = seen.clone();
    r.hooks
        .add_filter(
            &HookName::from("register_setting_args"),
            Callback::rust("see", move |args: &[HookValue]| {
                log.borrow_mut().extend(args.iter().cloned());
                Ok(args[0].clone())
            }),
            &HookValue::Int(10),
            4,
        )
        .unwrap();
    r.registry
        .register_setting(
            "g",
            "n",
            SettingArgsInput::Array(array(&[("label", s("L"))])),
        )
        .unwrap();
    let seen = seen.borrow();
    assert_eq!(seen[0], array(&[("label", s("L"))]));
    assert_eq!(
        keys(&seen[1]),
        [
            "type",
            "group",
            "label",
            "description",
            "sanitize_callback",
            "show_in_rest"
        ]
    );
    assert_eq!((seen[2].clone(), seen[3].clone()), (s("g"), s("n")));
}

/// Parses `type=integer&default=3` as PHP's `parse_str` would, for the query-string test.
struct QueryString;
impl ArgsParsing for QueryString {
    fn parse_args(&self, args: &HookValue) -> HookValue {
        match args {
            HookValue::Str(q) => HookValue::Array(
                q.split('&')
                    .filter_map(|pair| pair.split_once('='))
                    .map(|(k, v)| (Key::Str(k.into()), s(v)))
                    .collect(),
            ),
            _ => HookValue::Array(Vec::new()),
        }
    }
}

#[test]
fn query_string_args_reach_the_filter_raw_then_are_parsed() {
    let r = rig();
    let registry = SettingsRegistry::new(r.store.clone(), r.hooks.clone(), r.notices.clone())
        .with_args_parsing(Rc::new(QueryString));
    let seen: Rc<RefCell<Vec<HookValue>>> = Rc::default();
    let log = seen.clone();
    r.hooks
        .add_filter(
            &HookName::from("register_setting_args"),
            Callback::rust("see", move |args: &[HookValue]| {
                log.borrow_mut().push(args[0].clone());
                Ok(args[0].clone())
            }),
            &HookValue::Int(10),
            1,
        )
        .unwrap();
    registry
        .register_setting(
            "g",
            "q",
            SettingArgsInput::Other(s("type=integer&default=3")),
        )
        .unwrap();
    assert_eq!(seen.borrow()[0], s("type=integer&default=3"));
    let registered = r.store.get("q").unwrap();
    assert_eq!(field(&registered, "type"), Some(s("integer")));
    assert_eq!(field(&registered, "default"), Some(s("3")));
}

#[test]
fn deprecated_groups() {
    let r = rig();
    r.registry
        .register_setting("misc", "a", SettingArgsInput::Array(array(&[])))
        .unwrap();
    r.registry
        .register_setting("privacy", "b", SettingArgsInput::Array(array(&[])))
        .unwrap();
    assert_eq!(r.store.allowed("general"), vec![(0, "a".to_string())]);
    assert_eq!(r.store.allowed("reading"), vec![(0, "b".to_string())]);
    let notices = r.notices.deprecated.borrow();
    assert_eq!(notices.len(), 2);
    assert_eq!(
        (notices[0].0.as_str(), notices[0].1.as_str()),
        ("register_setting", "3.0.0")
    );
    assert_eq!(
        notices[0].2,
        "The \"misc\" options group has been removed. Use another settings group."
    );
    assert_eq!(
        (notices[1].0.as_str(), notices[1].1.as_str()),
        ("register_setting", "3.5.0")
    );
}

#[test]
fn array_type_without_items_is_doing_it_wrong() {
    let r = rig();
    let args = array(&[
        ("type", s("array")),
        ("show_in_rest", HookValue::Bool(true)),
    ]);
    r.registry
        .register_setting("g", "n", SettingArgsInput::Array(args))
        .unwrap();
    let wrong = r.notices.wrong.borrow();
    assert_eq!(wrong.len(), 1);
    assert_eq!(wrong[0].0, "register_setting");
    assert_eq!(
        wrong[0].1,
        "When registering an \"array\" setting to show in the REST API, you must specify the schema for each array \
         item in \"show_in_rest.schema.items\"."
    );
    assert_eq!(wrong[0].2, "5.4.0");
}

#[test]
fn unregister_removes_filters_and_fires() {
    let r = rig();
    let cb = Callback::rust("smarten", |args: &[HookValue]| Ok(args[0].clone()));
    r.registry
        .register_setting("g", "n", SettingArgsInput::Callable(cb))
        .unwrap();
    r.registry
        .register_setting(
            "g",
            "d",
            SettingArgsInput::Array(array(&[("default", s("x"))])),
        )
        .unwrap();
    let fired: Rc<RefCell<Vec<Vec<HookValue>>>> = Rc::default();
    let log = fired.clone();
    r.hooks
        .add_filter(
            &HookName::from("unregister_setting"),
            Callback::rust("fired", move |args: &[HookValue]| {
                log.borrow_mut().push(args.to_vec());
                Ok(HookValue::Null)
            }),
            &HookValue::Int(10),
            2,
        )
        .unwrap();
    r.registry.unregister_setting("g", "n", None).unwrap();
    r.registry.unregister_setting("g", "d", None).unwrap();
    let none = |hook: &str| {
        r.hooks
            .has_filter(&HookName::from(hook), None, None)
            .unwrap()
    };
    assert!(matches!(none("sanitize_option_n"), HasFilterResult::No));
    assert!(matches!(none("default_option_d"), HasFilterResult::No));
    assert_eq!(
        *fired.borrow(),
        vec![vec![s("g"), s("n")], vec![s("g"), s("d")]]
    );
    assert!(r.registry.registered_settings().is_empty());
}

#[test]
fn unregister_unknown_is_silent() {
    let r = rig();
    let fired: Rc<RefCell<u32>> = Rc::default();
    let count = fired.clone();
    r.hooks
        .add_filter(
            &HookName::from("unregister_setting"),
            Callback::rust("fired", move |_: &[HookValue]| {
                *count.borrow_mut() += 1;
                Ok(HookValue::Null)
            }),
            &HookValue::Int(10),
            2,
        )
        .unwrap();
    r.registry.unregister_setting("g", "unknown", None).unwrap();
    assert_eq!(*fired.borrow(), 0);
}

#[test]
fn allowed_keeps_php_indices() {
    let r = rig();
    for name in ["a", "b", "c"] {
        r.registry
            .register_setting("g", name, SettingArgsInput::Array(array(&[])))
            .unwrap();
    }
    r.registry.unregister_setting("g", "b", None).unwrap();
    assert_eq!(
        r.store.allowed("g"),
        vec![(0, "a".to_string()), (2, "c".to_string())]
    );
}
