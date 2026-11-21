//! The settings functions, native. `register_setting`'s `$args`: a callable is its sanitize callback; an array is
//! taken as it is; anything else reaches `register_setting_args` as given and is then parsed by `wp_parse_args`
//! (`PhpArgsParsing`), as in WordPress. A `sanitize_callback` stays a PHP value through the hooks, so PHP code hooked
//! on `register_setting_args` or `register_setting` sees it as given; `PhpArgsParsing` makes it a `Callback` only when
//! the registry adds it as a filter.

use speedyspec_php_ext::{Key, Value};
use speedyspec_wp_hooks::Callback;
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

fn args_input(args: Value) -> SettingArgsInput {
    // WordPress: `if ( is_callable( $args ) ) { $args = array( 'sanitize_callback' => $args ); }`.
    if callable(&args) {
        return SettingArgsInput::Array(Value::Array(vec![(
            Key::Str("sanitize_callback".into()),
            args,
        )]));
    }
    match args.read().into_owned() {
        array @ Value::Array(_) => SettingArgsInput::Array(array),
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
            Ok(array @ Value::Array(_)) => array,
            _ => Value::Array(Vec::new()),
        }
    }

    /// A `Callback` already made, else the PHP callable the value is.
    fn sanitize_callback(&self, value: &Value) -> Option<Callback> {
        match value {
            Value::Opaque(any) if any.is::<Callback>() => any.downcast_ref::<Callback>().cloned(),
            other => callable(other).then(|| php_callback(other.clone())),
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
