//! The options API's ports over PHP's WordPress functions, with WordPress's fallbacks when one is absent.
//!
//! `wp_installing()`, `defined('WP_SETUP_CONFIG')` and `is_multisite()` are read once and kept in a [`Snapshot`]
//! that every native function clears when PHP calls it, so a Rust caller between PHP calls reads without entering
//! PHP, and PHP's own changes are seen at its next call.

use std::cell::Cell;
use std::rc::Rc;

use speedyspec_php_ext::Value;
use speedyspec_wp_hooks::HookApi;
use speedyspec_wp_options::{
    CORE_SANITIZED_OPTIONS, Deprecations, HookSanitizing, Installing, Multisite, OptionGroups,
    OptionName, OptionsError, ProtectedOptions, Sanitizing,
};

/// Calls a PHP function; a PHP exception is `OptionsError::Php(exception)`, PHP unwinding (an `exit()`) is
/// `OptionsError::Php(Value::Null)`.
pub(crate) fn call(name: &str, args: &[Value]) -> Result<Value, OptionsError> {
    speedyspec_php::call_catching(&Value::Str(name.into()), args)
        .map_err(|failure| OptionsError::Php(failure.exception.unwrap_or(Value::Null)))
}

pub(crate) fn exists(function: &str) -> Result<bool, OptionsError> {
    Ok(matches!(
        call("function_exists", &[Value::Str(function.into())])?,
        Value::Bool(true)
    ))
}

fn truthy(v: &Value) -> bool {
    !speedyspec_wp_options::php_empty(&v.read())
}

/// PHP state read once between PHP calls.
#[derive(Default)]
pub struct Snapshot {
    installing: Cell<Option<bool>>,
    setup_config: Cell<Option<bool>>,
    multisite: Cell<Option<bool>>,
}

impl Snapshot {
    /// Forgets what was read: PHP is running and may change it.
    pub fn clear(&self) {
        self.installing.set(None);
        self.setup_config.set(None);
        self.multisite.set(None);
    }

    fn get(cell: &Cell<Option<bool>>, read: impl FnOnce() -> bool) -> bool {
        match cell.get() {
            Some(v) => v,
            None => {
                let v = read();
                cell.set(Some(v));
                v
            }
        }
    }
}

/// `wp_installing()` and `defined('WP_SETUP_CONFIG')`.
pub struct PhpInstalling(pub Rc<Snapshot>);

impl Installing for PhpInstalling {
    fn installing(&self) -> bool {
        Snapshot::get(&self.0.installing, || {
            exists("wp_installing").unwrap_or(false)
                && call("wp_installing", &[]).is_ok_and(|v| truthy(&v))
        })
    }

    fn setup_config(&self) -> bool {
        Snapshot::get(&self.0.setup_config, || {
            call("defined", &[Value::Str("WP_SETUP_CONFIG".into())]).is_ok_and(|v| truthy(&v))
        })
    }
}

/// `is_multisite()`, `get_current_blog_id()` and `get_current_network_id()`.
pub struct PhpMultisite(pub Rc<Snapshot>);

fn id(function: &str) -> u64 {
    if !exists(function).unwrap_or(false) {
        return 1;
    }
    match call(function, &[]) {
        Ok(Value::Int(i)) => u64::try_from(i).unwrap_or(1),
        _ => 1,
    }
}

impl Multisite for PhpMultisite {
    fn is_multisite(&self) -> bool {
        Snapshot::get(&self.0.multisite, || {
            exists("is_multisite").unwrap_or(false)
                && call("is_multisite", &[]).is_ok_and(|v| truthy(&v))
        })
    }

    fn blog_id(&self) -> u64 {
        id("get_current_blog_id")
    }

    fn network_id(&self) -> u64 {
        id("get_current_network_id")
    }

    fn is_main_site(&self) -> bool {
        !exists("is_main_site").unwrap_or(false)
            || call("is_main_site", &[]).is_ok_and(|v| truthy(&v))
    }

    fn is_main_network(&self) -> bool {
        !exists("is_main_network").unwrap_or(false)
            || call("is_main_network", &[]).is_ok_and(|v| truthy(&v))
    }
}

/// `sanitize_option()`: PHP's for the names core sanitizes; the filter alone, in Rust, for every other name.
pub struct PhpSanitizing(pub HookSanitizing);

impl PhpSanitizing {
    pub fn new(hooks: Rc<HookApi>) -> PhpSanitizing {
        PhpSanitizing(HookSanitizing::new(hooks))
    }
}

impl Sanitizing for PhpSanitizing {
    fn sanitize_option(&self, name: &OptionName, value: Value) -> Result<Value, OptionsError> {
        if CORE_SANITIZED_OPTIONS.contains(&name.as_str()) && exists("sanitize_option")? {
            return call(
                "sanitize_option",
                &[Value::Str(name.as_str().into()), value],
            );
        }
        match self.0.sanitize_option(name, value.clone()) {
            // Core's rules without formatting.php loaded: the value stays as given.
            Err(OptionsError::NeedsPhp { .. }) => Ok(value),
            other => other,
        }
    }
}

/// `_deprecated_argument()` and `_doing_it_wrong()` when defined.
pub struct PhpDeprecations;

impl Deprecations for PhpDeprecations {
    fn deprecated_argument(
        &self,
        function: &str,
        version: &str,
        message: &str,
    ) -> Result<(), OptionsError> {
        if exists("_deprecated_argument")? {
            call(
                "_deprecated_argument",
                &[
                    Value::Str(function.into()),
                    Value::Str(version.into()),
                    Value::Str(message.into()),
                ],
            )?;
        }
        Ok(())
    }

    fn doing_it_wrong(
        &self,
        function: &str,
        message: &str,
        version: &str,
    ) -> Result<(), OptionsError> {
        if exists("_doing_it_wrong")? {
            call(
                "_doing_it_wrong",
                &[
                    Value::Str(function.into()),
                    Value::Str(message.into()),
                    Value::Str(version.into()),
                ],
            )?;
        }
        Ok(())
    }
}

/// `wp_protect_special_option()`: `wp_die()` with WordPress's message.
pub struct PhpProtectedOptions;

impl ProtectedOptions for PhpProtectedOptions {
    fn protect(&self, name: &str) -> Result<(), OptionsError> {
        if name != "alloptions" && name != "notoptions" {
            return Ok(());
        }
        let template = "%s is a protected WP option and may not be modified";
        let template = if exists("__")? {
            match call("__", &[Value::Str(template.into())])? {
                Value::Str(s) => s,
                _ => template.into(),
            }
        } else {
            template.into()
        };
        let escaped = if exists("esc_html")? {
            match call("esc_html", &[Value::Str(name.into())])? {
                Value::Str(s) => s,
                _ => name.into(),
            }
        } else {
            name.into()
        };
        let message = template.replacen("%s", &escaped, 1);
        if exists("wp_die")? {
            call("wp_die", &[Value::Str(message.clone())])?;
        }
        // `wp_die()` returned (or is absent): the write must still not happen.
        Err(OptionsError::Protected(message))
    }
}

/// `$GLOBALS['new_allowed_options'][$group]`.
pub struct PhpOptionGroups;

impl OptionGroups for PhpOptionGroups {
    fn options_in(&self, group: &str) -> Option<Vec<String>> {
        match call(
            "SpeedySpec\\WpOptions\\allowed_options",
            &[Value::Str(group.into())],
        )
        .ok()?
        {
            Value::Array(items) => Some(
                items
                    .into_iter()
                    .filter_map(|(_, v)| speedyspec_wp_options::php_string(&v))
                    .map(|b| String::from_utf8_lossy(&b).into_owned())
                    .collect(),
            ),
            _ => None,
        }
    }
}
