//! The password ports over PHP and the Hook API (Adapters).

use std::rc::{Rc, Weak};

use speedyspec_container::Container;
use speedyspec_hooks::{Key, Value};
use speedyspec_php::PhpError;
use speedyspec_php_ext::PhpObject;
use speedyspec_wp_hooks::HookApi;
use speedyspec_wp_hooks_contracts::{Count, HookError, HookName};
use speedyspec_wp_password::{php_long, php_truthy};
use speedyspec_wp_password_contracts::{
    CostLimits, PASSWORD_COST_LIMITS, PasswordChecker, PasswordCostLimits, PasswordError,
    PasswordFilters, PasswordGenerator, PasswordHasher, RandomNumber, RehashPolicy, Warnings,
    WpHasherGlobal,
};

/// Bytes as a PHP string value.
pub(crate) fn php_string(bytes: &[u8]) -> Value {
    match std::str::from_utf8(bytes) {
        Ok(s) => Value::Str(s.to_owned()),
        Err(_) => Value::Bytes(bytes.to_vec()),
    }
}

/// A Hook API failure as a password error: a callback's exception is rethrown unchanged, and PHP unwinding stays
/// silent.
pub(crate) fn hook_error(err: HookError) -> PasswordError {
    match &err.payload {
        Some(payload) => match payload.downcast_ref::<Value>() {
            Some(exception) => PasswordError::Rethrow(exception.clone()),
            None => PasswordError::Unwinding,
        },
        None => PasswordError::Throw {
            class: "Error".into(),
            message: err.message,
        },
    }
}

/// Calls `callable(...$args)`, keeping the exception PHP throws; `exit()` and fatal errors stay silent.
fn call(callable: &Value, args: &[Value]) -> Result<Value, PasswordError> {
    speedyspec_php::call_catching(callable, args).map_err(|failure| match failure.exception {
        Some(exception) => PasswordError::Rethrow(exception),
        None => match failure.error {
            PhpError::Exit(_) | PhpError::Fatal { .. } => PasswordError::Unwinding,
            other => PasswordError::Port(other.to_string()),
        },
    })
}

/// Calls `$target->$method(...$args)`, keeping the exception PHP throws.
fn call_method(target: &Value, method: &str, args: &[Value]) -> Result<Value, PasswordError> {
    let callable = Value::Array(vec![
        (Key::Int(0), target.clone()),
        (Key::Int(1), Value::Str(method.to_owned())),
    ]);
    call(&callable, args)
}

/// WordPress's five password filters over the Hook API. With no callback a run is only counted and PHP is not
/// entered.
///
/// The Hook API is held weakly. These filters live in container bindings, and the Hook API reaches the container
/// through its hook store, so a strong reference would make a cycle (Container → binding → filters → Hook API →
/// store → Container). Whoever built the filters keeps the Hook API alive ([`crate::PasswordArea`] does, for the
/// worker).
pub struct HookPasswordFilters {
    hooks: Weak<HookApi>,
}

impl HookPasswordFilters {
    /// Filters through `hooks`, without keeping it alive.
    pub fn new(hooks: &Rc<HookApi>) -> HookPasswordFilters {
        HookPasswordFilters {
            hooks: Rc::downgrade(hooks),
        }
    }

    fn apply(&self, name: &str, value: Value, args: Vec<Value>) -> Result<Value, PasswordError> {
        let hooks = self
            .hooks
            .upgrade()
            .ok_or_else(|| PasswordError::Port("the Hook API is gone".into()))?;
        let name = HookName::from(name);
        if hooks
            .skip_if_empty(&name, Count::Filters)
            .map_err(hook_error)?
        {
            return Ok(value);
        }
        hooks.apply_filters(&name, value, args).map_err(hook_error)
    }
}

impl PasswordFilters for HookPasswordFilters {
    fn algorithm(&self, default: Value) -> Result<Value, PasswordError> {
        self.apply("wp_hash_password_algorithm", default, Vec::new())
    }

    fn options(&self, options: Value, algorithm: &Value) -> Result<Value, PasswordError> {
        self.apply("wp_hash_password_options", options, vec![algorithm.clone()])
    }

    fn check_password(
        &self,
        check: Value,
        password: &[u8],
        hash: &[u8],
        user_id: &Value,
    ) -> Result<Value, PasswordError> {
        self.apply(
            "check_password",
            check,
            vec![php_string(password), php_string(hash), user_id.clone()],
        )
    }

    fn needs_rehash(
        &self,
        needs: bool,
        hash: &[u8],
        user_id: &Value,
    ) -> Result<Value, PasswordError> {
        self.apply(
            "password_needs_rehash",
            Value::Bool(needs),
            vec![php_string(hash), user_id.clone()],
        )
    }

    fn random_password(
        &self,
        password: Vec<u8>,
        length: &Value,
        special: &Value,
        extra: &Value,
    ) -> Result<Value, PasswordError> {
        self.apply(
            "random_password",
            php_string(&password),
            vec![length.clone(), special.clone(), extra.clone()],
        )
    }
}

/// `$wp_hasher`, read from PHP's globals when WordPress would read it.
pub struct PhpWpHasherGlobal;

impl PhpWpHasherGlobal {
    /// The hasher, when `! empty( $wp_hasher )`.
    fn hasher() -> Option<Value> {
        speedyspec_php::global("wp_hasher").filter(php_truthy)
    }
}

impl WpHasherGlobal for PhpWpHasherGlobal {
    fn is_set(&self) -> bool {
        Self::hasher().is_some()
    }

    fn hash(&self, trimmed: &[u8]) -> Option<Result<Value, PasswordError>> {
        Self::hasher().map(|hasher| call_method(&hasher, "HashPassword", &[php_string(trimmed)]))
    }

    fn check(&self, password: &[u8], hash: &[u8]) -> Option<Result<Value, PasswordError>> {
        Self::hasher().map(|hasher| {
            call_method(
                &hasher,
                "CheckPassword",
                &[php_string(password), php_string(hash)],
            )
        })
    }
}

/// PHP warnings from `password_hash()`'s rules, raised with PHP's text through `trigger_error()`.
pub struct PhpWarnings;

impl Warnings for PhpWarnings {
    fn warn(&self, message: &str) -> Result<(), PasswordError> {
        // E_USER_WARNING: a native function has no way to raise E_WARNING itself. An error handler may throw or
        // exit(); either is passed on.
        call(
            &Value::Str("trigger_error".into()),
            &[Value::Str(message.to_owned()), Value::Int(512)],
        )
        .map(drop)
    }
}

/// A PHP object implementing one of the twin interfaces.
pub struct PhpReplacement(PhpObject);

impl PhpReplacement {
    pub fn new(object: PhpObject) -> PhpReplacement {
        PhpReplacement(object)
    }

    fn call(&self, method: &str, args: &[Value]) -> Result<Value, PasswordError> {
        call_method(self.0.value(), method, args)
    }
}

impl PasswordHasher for PhpReplacement {
    fn hash(&self, password: &[u8]) -> Result<Value, PasswordError> {
        self.call("hash", &[php_string(password)])
    }
}

impl PasswordChecker for PhpReplacement {
    fn check(&self, password: &[u8], hash: &[u8], user_id: &Value) -> Result<Value, PasswordError> {
        self.call(
            "check",
            &[php_string(password), php_string(hash), user_id.clone()],
        )
    }
}

impl RehashPolicy for PhpReplacement {
    fn needs_rehash(&self, hash: &[u8], user_id: &Value) -> Result<Value, PasswordError> {
        self.call("needs_rehash", &[php_string(hash), user_id.clone()])
    }
}

impl PasswordGenerator for PhpReplacement {
    fn generate(
        &self,
        length: &Value,
        special: &Value,
        extra: &Value,
    ) -> Result<Value, PasswordError> {
        self.call(
            "generate",
            &[length.clone(), special.clone(), extra.clone()],
        )
    }
}

impl RandomNumber for PhpReplacement {
    fn rand(&self, min: i64, max: i64) -> Result<Value, PasswordError> {
        self.call("rand", &[Value::Int(min), Value::Int(max)])
    }
}

impl PasswordCostLimits for PhpReplacement {
    fn limits(&self) -> Result<CostLimits, PasswordError> {
        cost_limits(&self.call("limits", &[])?)
    }
}

/// The limits a PHP `limits()` returned: `cost`, `memory_cost` and `time_cost`, each defaulting to
/// [`CostLimits::DEFAULT`]'s and coerced as `password_hash()` coerces its options.
fn cost_limits(value: &Value) -> Result<CostLimits, PasswordError> {
    const METHOD: &str = "SpeedySpec\\WpPassword\\PasswordCostLimits::limits()";
    let value = &*value.read();
    let Value::Array(entries) = value else {
        return Err(PasswordError::type_error(format!(
            "{METHOD}: Return value must be of type array, {} given",
            speedyspec_wp_password::php_type_name(value)
        )));
    };
    let read = |name: &str, default: u32, min: u32, max: u32| -> Result<u32, PasswordError> {
        let Some((_, v)) = entries
            .iter()
            .find(|(key, _)| matches!(key, Key::Str(k) if k == name))
        else {
            return Ok(default);
        };
        let n = php_long(v);
        u32::try_from(n)
            .ok()
            .filter(|n| (min..=max).contains(n))
            .ok_or_else(|| {
                PasswordError::value_error(format!(
                    "{METHOD}: \"{name}\" must be between {min} and {max}, {n} given"
                ))
            })
    };
    let d = CostLimits::DEFAULT;
    Ok(CostLimits {
        bcrypt_cost: read("cost", d.bcrypt_cost, 4, 31)?,
        argon2_memory_kib: read("memory_cost", d.argon2_memory_kib, 8, u32::MAX)?,
        argon2_time_cost: read("time_cost", d.argon2_time_cost, 1, u32::MAX)?,
    })
}

/// The site's cost limits, resolved from the container each time they're read, so a binding made for the site or
/// the request (in PHP, `app.<id>.php.bindings` or the container) applies. The container is held weakly: it holds
/// this adapter's owners.
pub struct ResolvedCostLimits {
    container: Weak<Container>,
}

impl ResolvedCostLimits {
    /// Limits resolved from `container`, without keeping it alive.
    pub fn new(container: &Rc<Container>) -> ResolvedCostLimits {
        ResolvedCostLimits {
            container: Rc::downgrade(container),
        }
    }
}

impl PasswordCostLimits for ResolvedCostLimits {
    fn limits(&self) -> Result<CostLimits, PasswordError> {
        self.container
            .upgrade()
            .ok_or_else(|| PasswordError::Port("the container is gone".into()))?
            .make_port::<dyn PasswordCostLimits>(PASSWORD_COST_LIMITS)
            .map_err(|err| PasswordError::Port(err.to_string()))?
            .limits()
    }
}
