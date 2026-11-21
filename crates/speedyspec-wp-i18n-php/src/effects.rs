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
