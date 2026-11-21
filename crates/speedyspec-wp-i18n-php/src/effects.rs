//! PHP adapters for the domain's locale ports: switching effects run WordPress's loaders and fire WordPress's
//! actions; errors keep the PHP exception they came from, so the native function rethrows it unchanged.

use std::fmt;
use std::rc::Rc;

use speedyspec_hooks::Value;
use speedyspec_php::native::NativeError;
use speedyspec_wp_i18n::{I18nApi, I18nError};
use speedyspec_wp_i18n_contracts::{PhpValue, PortError, SwitchEffects};

/// A PHP call that failed, with the exception object PHP threw (when it threw one).
#[derive(Debug)]
pub(crate) struct PhpFailure {
    message: String,
    exception: Option<Value>,
}

impl fmt::Display for PhpFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for PhpFailure {}

/// Calls a PHP function by name, as WordPress code would call it.
pub(crate) fn call(function: &str, args: &[Value]) -> Result<Value, PortError> {
    speedyspec_php::call_catching(&Value::Str(function.to_owned()), args).map_err(|f| {
        PortError(Box::new(PhpFailure {
            message: f.error.to_string(),
            exception: f.exception,
        }))
    })
}

/// The PHP error a failed native call raises: the original exception when there is one.
pub(crate) fn native_error(err: I18nError) -> NativeError {
    match err {
        I18nError::Hook(h) => {
            if let Some(exception) = h.payload.as_ref().and_then(|p| p.downcast_ref::<Value>()) {
                return NativeError::Rethrow(exception.clone());
            }
            NativeError::throw("Error", h.message)
        }
        I18nError::Port(p) => match p.0.downcast_ref::<PhpFailure>() {
            Some(PhpFailure {
                exception: Some(e), ..
            }) => NativeError::Rethrow(e.clone()),
            _ => NativeError::throw("Error", p.to_string()),
        },
        I18nError::Plural(e) => NativeError::throw(e.php_class(), e.to_string()),
        I18nError::Type(m) => NativeError::throw("TypeError", m),
    }
}

/// `WP_Locale_Switcher`'s effects in a PHP request.
pub(crate) struct PhpSwitchEffects {
    pub api: Rc<I18nApi>,
}

fn hook(e: I18nError) -> PortError {
    PortError(Box::new(HookFailure(e)))
}

/// A hook failure carried through the port, unwrapped again by [`port_error`].
#[derive(Debug)]
pub(crate) struct HookFailure(pub I18nError);

impl fmt::Display for HookFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl std::error::Error for HookFailure {}

/// A port error as the native function raises it.
pub(crate) fn port_error(err: PortError) -> NativeError {
    match err.0.downcast::<HookFailure>() {
        Ok(h) => native_error(h.0),
        Err(other) => native_error(I18nError::Port(PortError(other))),
    }
}

impl SwitchEffects for PhpSwitchEffects {
    fn determine_locale(&self) -> Result<PhpValue, PortError> {
        call("determine_locale", &[])
    }

    fn user_locale(&self, user: &PhpValue) -> Result<PhpValue, PortError> {
        call("get_user_locale", std::slice::from_ref(user))
    }

    fn change_locale(&self, locale: &PhpValue) -> Result<(), PortError> {
        call(
            "SpeedySpec\\WpI18n\\switcher_change_locale",
            std::slice::from_ref(locale),
        )?;
        self.api
            .action("change_locale", vec![locale.clone()])
            .map_err(hook)
    }

    fn switched(&self, locale: &PhpValue, user: &PhpValue) -> Result<(), PortError> {
        self.api
            .action("switch_locale", vec![locale.clone(), user.clone()])
            .map_err(hook)
    }

    fn restored(&self, locale: &PhpValue, previous: &PhpValue) -> Result<(), PortError> {
        self.api
            .action(
                "restore_previous_locale",
                vec![locale.clone(), previous.clone()],
            )
            .map_err(hook)
    }
}

/// A PHP value as PHP's string conversion gives it, for the scalar values these ports read.
pub(crate) fn php_string(v: &Value) -> String {
    match v {
        Value::Str(s) => s.clone(),
        Value::Bytes(b) => String::from_utf8_lossy(b).into_owned(),
        Value::Int(n) => n.to_string(),
        Value::Float(f) => f.to_string(),
        Value::Bool(true) => "1".into(),
        _ => String::new(),
    }
}

/// What `_load_script_textdomain_from_src()` asks WordPress, in a PHP request.
pub(crate) struct PhpScriptSources;

impl speedyspec_wp_i18n_contracts::ScriptSources for PhpScriptSources {
    fn determine_locale(&self) -> Result<String, PortError> {
        call("determine_locale", &[]).map(|v| php_string(&v))
    }

    fn registry_path(&self, domain: &str, locale: &str) -> Result<Option<String>, PortError> {
        let v = call(
            "SpeedySpec\\WpI18n\\registry_get",
            &[Value::Str(domain.to_owned()), Value::Str(locale.to_owned())],
        )?;
        Ok(match v {
            Value::Bool(false) | Value::Null => None,
            other => Some(php_string(&other)),
        })
    }

    fn parse_url(
        &self,
        url: &str,
    ) -> Result<Option<speedyspec_wp_i18n_contracts::UrlParts>, PortError> {
        let v = call("wp_parse_url", &[Value::Str(url.to_owned())])?;
        let Value::Array(items) = v else {
            return Ok(None);
        };
        let part = |name: &str| {
            items.iter().find_map(|(k, v)| match (k, v) {
                (speedyspec_hooks::Key::Str(k), Value::Null) if k == name => None,
                (speedyspec_hooks::Key::Str(k), v) if k == name => Some(php_string(v)),
                _ => None,
            })
        };
        Ok(Some(speedyspec_wp_i18n_contracts::UrlParts {
            scheme: part("scheme"),
            host: part("host"),
            path: part("path"),
        }))
    }

    fn content_url(&self) -> Result<String, PortError> {
        call("content_url", &[]).map(|v| php_string(&v))
    }

    fn plugins_url(&self) -> Result<String, PortError> {
        call("plugins_url", &[]).map(|v| php_string(&v))
    }

    fn site_url(&self) -> Result<String, PortError> {
        call("site_url", &[]).map(|v| php_string(&v))
    }

    fn theme_root(&self) -> Result<String, PortError> {
        call("get_theme_root", &[]).map(|v| php_string(&v))
    }

    fn lang_dir(&self) -> String {
        call("constant", &[Value::Str("WP_LANG_DIR".into())])
            .map(|v| php_string(&v))
            .unwrap_or_default()
    }
}
