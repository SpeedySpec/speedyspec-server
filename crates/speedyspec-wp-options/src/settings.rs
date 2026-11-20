//! Registered settings (`option.php` lines 2997–3256): `register_setting()` and its registry (Registry), whose state
//! lives behind the `SettingsStore` port, as hooks live behind `HookStore`. In PHP the store *is*
//! `$wp_registered_settings` and `$new_allowed_options`.

use std::rc::Rc;

use indexmap::IndexMap;
use speedyspec_wp_hooks::{Callback, CallbackId, HookApi, HookName, HookValue, Key};

use crate::{Deprecations, OptionsError, php_empty};

/// The registered settings and the allowed options per group.
pub trait SettingsStore {
    fn registered(&self) -> IndexMap<String, HookValue>;
    fn get(&self, name: &str) -> Option<HookValue>;
    fn put(&self, name: &str, args: HookValue);
    fn remove(&self, name: &str);
    /// `$new_allowed_options[ $group ][] = $name`.
    fn allow(&self, group: &str, name: &str);
    /// `array_search( $name, $new_allowed_options[ $group ], true )`, then `unset`: PHP's index holes stay.
    fn disallow(&self, group: &str, name: &str);
}

/// `register_setting()`'s `$args`: an array, a callable that becomes its `sanitize_callback`, or anything else (a
/// query string), which `register_setting_args` sees as given and `wp_parse_args` then parses.
#[derive(Clone, Debug)]
pub enum SettingArgsInput {
    Array(HookValue),
    Callable(Callback),
    Other(HookValue),
}

/// How the host language reads `register_setting`'s arguments (Strategy): `wp_parse_args()` of arguments that are not
/// an array (a query string, through PHP's `parse_str`), and the callback a parsed `sanitize_callback` names. The
/// arguments pass through `register_setting_args` and `register_setting` as given, so a PHP callable stays a PHP
/// value until the registry adds it as a filter.
pub trait ArgsParsing {
    fn parse_args(&self, args: &HookValue) -> HookValue;

    /// The callback a non-empty `sanitize_callback` names: by default, a `Callback` carried opaquely.
    fn sanitize_callback(&self, value: &HookValue) -> Option<Callback> {
        callback_of(value)
    }
}

/// Without PHP, arguments that are not an array count as none.
pub struct NoArgsParsing;

impl ArgsParsing for NoArgsParsing {
    fn parse_args(&self, _args: &HookValue) -> HookValue {
        HookValue::Array(Vec::new())
    }
}

/// The function name `register_setting()` adds to `default_option_{$name}`.
pub const FILTER_DEFAULT_OPTION: &str = "filter_default_option";

fn s(v: &str) -> HookValue {
    HookValue::Str(v.to_string())
}

fn entry<'a>(array: &'a HookValue, name: &str) -> Option<&'a HookValue> {
    match array {
        HookValue::Array(items) => items
            .iter()
            .find(|(k, _)| *k == Key::Str(name.into()))
            .map(|(_, v)| v),
        _ => None,
    }
}

/// `wp_parse_args( $args, $defaults )`: the defaults' keys first (the args' values winning), then the args' other
/// keys in their order. Arguments that are not an array go through `parsing` first.
fn parse_args(args: &HookValue, defaults: &HookValue, parsing: &dyn ArgsParsing) -> HookValue {
    let (HookValue::Array(defaults), args) = (defaults, args.read().into_owned()) else {
        unreachable!("the defaults are an array")
    };
    let args = match args {
        HookValue::Array(items) => items,
        other => match parsing.parse_args(&other).read().into_owned() {
            HookValue::Array(items) => items,
            _ => Vec::new(),
        },
    };
    let mut out = defaults.clone();
    for (k, v) in args {
        match out.iter_mut().find(|(key, _)| *key == k) {
            Some(slot) => slot.1 = v,
            None => out.push((k, v)),
        }
    }
    HookValue::Array(out)
}

/// The callback a `sanitize_callback` entry holds: a `Callback` carried opaquely (the PHP adapter makes one of a
/// PHP callable).
fn callback_of(value: &HookValue) -> Option<Callback> {
    match value {
        HookValue::Opaque(any) => any.downcast_ref::<Callback>().cloned(),
        _ => None,
    }
}

/// The removed group's replacement, its deprecation version and WordPress's message.
fn removed_group(group: &str) -> Option<(&'static str, &'static str, String)> {
    let message = |g: &str| {
        format!("The \"{g}\" options group has been removed. Use another settings group.")
    };
    match group {
        "misc" => Some(("general", "3.0.0", message("misc"))),
        "privacy" => Some(("reading", "3.5.0", message("privacy"))),
        _ => None,
    }
}

/// `filter_default_option( $default_value, $option, $passed_default )` over a store.
fn filter_default(
    store: &dyn SettingsStore,
    default: HookValue,
    option: &str,
    passed_default: bool,
) -> HookValue {
    if passed_default {
        return default;
    }
    match store.get(option) {
        Some(args) if !php_empty(&args) => {
            entry(&args, "default").cloned().unwrap_or(HookValue::Null)
        }
        _ => default,
    }
}

pub struct SettingsRegistry {
    store: Rc<dyn SettingsStore>,
    hooks: Rc<HookApi>,
    deprecations: Rc<dyn Deprecations>,
    parsing: Rc<dyn ArgsParsing>,
}

impl SettingsRegistry {
    pub fn new(
        store: Rc<dyn SettingsStore>,
        hooks: Rc<HookApi>,
        deprecations: Rc<dyn Deprecations>,
    ) -> SettingsRegistry {
        SettingsRegistry {
            store,
            hooks,
            deprecations,
            parsing: Rc::new(NoArgsParsing),
        }
    }

    /// Parses arguments that are not an array as WordPress's `wp_parse_args()` does (the PHP adapter).
    pub fn with_args_parsing(mut self, parsing: Rc<dyn ArgsParsing>) -> SettingsRegistry {
        self.parsing = parsing;
        self
    }

    /// `misc` and `privacy` are gone: deprecated, and replaced by `general` and `reading`.
    fn group(&self, function: &str, group: &str) -> Result<String, OptionsError> {
        match removed_group(group) {
            Some((replacement, version, message)) => {
                self.deprecations
                    .deprecated_argument(function, version, &message)?;
                Ok(replacement.to_string())
            }
            None => Ok(group.to_string()),
        }
    }

    /// `register_setting( $option_group, $option_name, $args )`.
    pub fn register_setting(
        &self,
        group: &str,
        name: &str,
        args: SettingArgsInput,
    ) -> Result<(), OptionsError> {
        let defaults = HookValue::Array(vec![
            (Key::Str("type".into()), s("string")),
            (Key::Str("group".into()), s(group)),
            (Key::Str("label".into()), s("")),
            (Key::Str("description".into()), s("")),
            (Key::Str("sanitize_callback".into()), HookValue::Null),
            (Key::Str("show_in_rest".into()), HookValue::Bool(false)),
        ]);
        let args = match args {
            SettingArgsInput::Array(v) | SettingArgsInput::Other(v) => v,
            SettingArgsInput::Callable(cb) => HookValue::Array(vec![(
                Key::Str("sanitize_callback".into()),
                HookValue::Opaque(Rc::new(cb)),
            )]),
        };
        let args = self.hooks.apply_filters(
            &HookName::from("register_setting_args"),
            args,
            vec![defaults.clone(), s(group), s(name)],
        )?;
        let args = parse_args(&args, &defaults, &*self.parsing);
        let show_in_rest = entry(&args, "show_in_rest")
            .cloned()
            .unwrap_or(HookValue::Bool(false));
        let is_array_type = entry(&args, "type").is_some_and(|t| *t == s("array"));
        if !matches!(show_in_rest, HookValue::Bool(false)) && is_array_type {
            let items = entry(&show_in_rest, "schema").and_then(|schema| entry(schema, "items"));
            if items.is_none_or(|i| matches!(i, HookValue::Null)) {
                self.deprecations.doing_it_wrong(
                    "register_setting",
                    "When registering an \"array\" setting to show in the REST API, you must specify the schema for \
                     each array item in \"show_in_rest.schema.items\".",
                    "5.4.0",
                )?;
            }
        }
        let group = self.group("register_setting", group)?;
        self.store.allow(&group, name);
        if let Some(sanitize) = entry(&args, "sanitize_callback").filter(|v| !php_empty(v))
            && let Some(cb) = self.parsing.sanitize_callback(sanitize)
        {
            self.hooks.add_filter(
                &HookName::from(format!("sanitize_option_{name}").as_str()),
                cb,
                &HookValue::Int(10),
                1,
            )?;
        }
        if entry(&args, "default").is_some() {
            let store = self.store.clone();
            let cb = Callback::rust(FILTER_DEFAULT_OPTION, move |a: &[HookValue]| {
                let default = a.first().cloned().unwrap_or(HookValue::Bool(false));
                let option = match a.get(1) {
                    Some(HookValue::Str(o)) => o.clone(),
                    _ => String::new(),
                };
                let passed = a.get(2).is_some_and(|p| !php_empty(p));
                Ok(filter_default(&*store, default, &option, passed))
            });
            self.hooks.add_filter(
                &HookName::from(format!("default_option_{name}").as_str()),
                cb,
                &HookValue::Int(10),
                3,
            )?;
        }
        self.hooks.do_action(
            &HookName::from("register_setting"),
            vec![s(&group), s(name), args.clone()],
        )?;
        self.store.put(name, args);
        Ok(())
    }

    /// `unregister_setting( $option_group, $option_name, $deprecated )`.
    pub fn unregister_setting(
        &self,
        group: &str,
        name: &str,
        deprecated: Option<Callback>,
    ) -> Result<(), OptionsError> {
        let group = self.group("unregister_setting", group)?;
        self.store.disallow(&group, name);
        let sanitize_hook = HookName::from(format!("sanitize_option_{name}").as_str());
        let ten = HookValue::Int(10);
        if let Some(cb) = deprecated {
            self.deprecations.deprecated_argument(
                "unregister_setting",
                "4.7.0",
                "<code>$sanitize_callback</code> is deprecated. The callback from <code>register_setting()</code> is \
                 used instead.",
            )?;
            self.hooks.remove_filter(
                &sanitize_hook,
                CallbackId(cb.id().map(str::to_string)),
                &ten,
            )?;
        }
        if let Some(args) = self.store.get(name) {
            if let Some(sanitize) = entry(&args, "sanitize_callback").filter(|v| !php_empty(v))
                && let Some(cb) = self.parsing.sanitize_callback(sanitize)
            {
                self.hooks.remove_filter(
                    &sanitize_hook,
                    CallbackId(cb.id().map(str::to_string)),
                    &ten,
                )?;
            }
            if entry(&args, "default").is_some() {
                self.hooks.remove_filter(
                    &HookName::from(format!("default_option_{name}").as_str()),
                    CallbackId(Some(FILTER_DEFAULT_OPTION.into())),
                    &ten,
                )?;
            }
            self.hooks.do_action(
                &HookName::from("unregister_setting"),
                vec![s(&group), s(name)],
            )?;
            self.store.remove(name);
        }
        Ok(())
    }

    /// `get_registered_settings()`.
    pub fn registered_settings(&self) -> IndexMap<String, HookValue> {
        self.store.registered()
    }

    /// `filter_default_option( $default_value, $option, $passed_default )`.
    pub fn filter_default_option(
        &self,
        default: HookValue,
        option: &str,
        passed_default: bool,
    ) -> HookValue {
        filter_default(&*self.store, default, option, passed_default)
    }
}
