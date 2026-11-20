//! The settings functions, native. `register_setting`'s `$args`: a callable is its sanitize callback; an array is
//! taken as it is (a callable `sanitize_callback` in it carried as a `Callback`); anything else reaches
//! `register_setting_args` as given and is then parsed by `wp_parse_args` (`PhpArgsParsing`), as in WordPress.

use std::rc::Rc;

use speedyspec_php_ext::{Key, Value};
use speedyspec_wp_hooks_php::php_callback;
use speedyspec_wp_options::{
    ArgsParsing, FilterDefaultOption, GetRegisteredSettings, RegisterSetting, SettingArgsInput,
    UnregisterSetting, php_empty,
};

use crate::adapters::{call, exists, truthy};
use crate::native::{Native, a, h, key, name, send};
use crate::ports::plain;

fn callable(v: &Value) -> bool {
    call("is_callable", std::slice::from_ref(v)).is_ok_and(|r| truthy(&r))
}

/// An args array with a non-empty `sanitize_callback` carried as a `Callback`, as the registry adds it.
fn with_callbacks(array: Vec<(Key, Value)>) -> Value {
    Value::Array(
        array
            .into_iter()
            .map(|(k, v)| {
                let sanitize =
                    matches!(&k, Key::Str(s) if s == "sanitize_callback") && !php_empty(&v.read());
                if sanitize {
                    (k, Value::Opaque(Rc::new(php_callback(v))))
                } else {
                    (k, v)
                }
            })
            .collect(),
    )
}

fn args_input(args: Value) -> SettingArgsInput {
    if callable(&args) {
        return SettingArgsInput::Callable(php_callback(args));
    }
    match args.read().into_owned() {
        Value::Array(items) => SettingArgsInput::Array(with_callbacks(items)),
        other => SettingArgsInput::Other(other),
    }
}

/// `wp_parse_args()` of arguments that are not an array, in PHP (a query string goes through `wp_parse_str`).
pub struct PhpArgsParsing;

impl ArgsParsing for PhpArgsParsing {
    fn parse_args(&self, args: &Value) -> Value {
        if !exists("wp_parse_args").unwrap_or(false) {
            return Value::Array(Vec::new());
        }
        match call("wp_parse_args", std::slice::from_ref(args)).map(|v| v.read().into_owned()) {
            Ok(Value::Array(items)) => with_callbacks(items),
            _ => Value::Array(Vec::new()),
        }
    }
}

pub(crate) fn entries() -> Vec<(&'static str, Native)> {
    vec![
        ("register_setting", |x| {
            let group = name(&a(x, 0)).unwrap_or_default();
            let option = name(&a(x, 1)).unwrap_or_default();
            let args = args_input(x.get(2).cloned().unwrap_or(Value::Array(Vec::new())));
            h(|h| {
                send(
                    &h.register_setting,
                    RegisterSetting {
                        group,
                        name: option,
                        args,
                    },
                )
            })??;
            Ok(Value::Null)
        }),
        ("unregister_setting", |x| {
            let group = name(&a(x, 0)).unwrap_or_default();
            let option = name(&a(x, 1)).unwrap_or_default();
            let deprecated = match x.get(2) {
                None => None,
                Some(Value::Str(s)) if s.is_empty() => None,
                Some(v) => Some(php_callback(v.clone())),
            };
            h(|h| {
                send(
                    &h.unregister_setting,
                    UnregisterSetting {
                        group,
                        name: option,
                        deprecated,
                    },
                )
            })??;
            Ok(Value::Null)
        }),
        ("get_registered_settings", |_| {
            let settings = h(|h| h.registered_settings.call(GetRegisteredSettings {}))?
                .map_err(|e| speedyspec_php::native::NativeError::throw("Error", e.to_string()))?;
            Ok(Value::Array(
                settings
                    .into_iter()
                    .map(|(n, v)| (key(&n), plain(v)))
                    .collect(),
            ))
        }),
        ("filter_default_option", |x| {
            let m = FilterDefaultOption {
                default: a(x, 0),
                option: name(&a(x, 1)).unwrap_or_default(),
                passed_default: !php_empty(&a(x, 2).read()),
            };
            h(|h| h.filter_default.call(m))?
                .map_err(|e| speedyspec_php::native::NativeError::throw("Error", e.to_string()))
        }),
    ]
}
