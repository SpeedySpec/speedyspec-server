//! The settings functions, native. `register_setting`'s `$args`: a callable is its sanitize callback; an array is
//! taken as it is (a callable `sanitize_callback` in it carried as a `Callback`); anything else goes through
//! `wp_parse_args` first, as WordPress's own `wp_parse_args( $args, $defaults )` would take it.

use std::rc::Rc;

use speedyspec_php_ext::{Key, Value};
use speedyspec_wp_hooks_php::php_callback;
use speedyspec_wp_options::{
    FilterDefaultOption, GetRegisteredSettings, RegisterSetting, SettingArgsInput,
    UnregisterSetting, php_empty,
};

use crate::adapters::{call, exists, truthy};
use crate::native::{Native, a, h, key, name, send};
use crate::ports::plain;

fn callable(v: &Value) -> bool {
    call("is_callable", std::slice::from_ref(v)).is_ok_and(|r| truthy(&r))
}

fn args_input(args: Value) -> SettingArgsInput {
    if callable(&args) {
        return SettingArgsInput::Callable(php_callback(args));
    }
    let array = match args.read().into_owned() {
        Value::Array(items) => items,
        other if exists("wp_parse_args").unwrap_or(false) => {
            match call("wp_parse_args", &[other]) {
                Ok(v) => match v.read().into_owned() {
                    Value::Array(items) => items,
                    _ => Vec::new(),
                },
                Err(_) => Vec::new(),
            }
        }
        _ => Vec::new(),
    };
    SettingArgsInput::Array(Value::Array(
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
    ))
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
