//! Contracts for WordPress's password functions: `wp_hash_password()`, `wp_check_password()`,
//! `wp_password_needs_rehash()`, `wp_generate_password()` and `wp_rand()`.
//!
//! WordPress makes each of these pluggable: a plugin replaces one by declaring it before `pluggable.php` does.
//! SpeedySpec replaces that mechanism with the service container. Each function is one port, so any one can be
//! replaced alone, as in WordPress:
//!
//! - [`PasswordHasher`] is `wp_hash_password()`, bound under [`PASSWORD_HASHER`];
//! - [`PasswordChecker`] is `wp_check_password()`, under [`PASSWORD_CHECKER`];
//! - [`RehashPolicy`] is `wp_password_needs_rehash()`, under [`REHASH_POLICY`];
//! - [`PasswordGenerator`] is `wp_generate_password()`, under [`PASSWORD_GENERATOR`];
//! - [`RandomNumber`] is `wp_rand()`, under [`RANDOM_NUMBER`].
//!
//! Their results are [`Value`]s: WordPress returns what its filters return, untouched, whatever its type.
//!
//! What WordPress's own implementations reach outside themselves are ports too: [`PasswordFilters`] (the five
//! filters, through the Hook API), [`WpHasherGlobal`] (a plugin's `$wp_hasher` object), [`Bcrypt`] and
//! [`Warnings`] (what PHP's `password_hash()` warns about). Passwords and hashes are bytes throughout, as PHP strings
//! are.

pub use speedyspec_hooks::Value;

/// The container key of [`PasswordHasher`]; also the name of its PHP twin interface.
pub const PASSWORD_HASHER: &str = "SpeedySpec\\WpPassword\\PasswordHasher";
/// The container key of [`PasswordChecker`].
pub const PASSWORD_CHECKER: &str = "SpeedySpec\\WpPassword\\PasswordChecker";
/// The container key of [`RehashPolicy`].
pub const REHASH_POLICY: &str = "SpeedySpec\\WpPassword\\RehashPolicy";
/// The container key of [`PasswordGenerator`].
pub const PASSWORD_GENERATOR: &str = "SpeedySpec\\WpPassword\\PasswordGenerator";
/// The container key of [`RandomNumber`].
pub const RANDOM_NUMBER: &str = "SpeedySpec\\WpPassword\\RandomNumber";

/// Why a password function didn't return.
#[derive(Clone, Debug, PartialEq)]
pub enum PasswordError {
    /// What PHP throws: `ValueError` and `TypeError` from `password_hash()`'s rules, or a filter callback's exception.
    Throw { class: String, message: String },
    /// A PHP exception object (a `Value::Opaque`) a filter callback or a replacement threw, to rethrow unchanged.
    Rethrow(Value),
    /// PHP is unwinding (`exit()` or a fatal error in a callback): nothing more runs, nothing is thrown.
    Unwinding,
    /// A replacement bound in the container failed in a way that isn't a PHP exception.
    Port(String),
}

impl PasswordError {
    /// A `ValueError` with PHP's message.
    pub fn value_error(message: impl Into<String>) -> PasswordError {
        PasswordError::Throw {
            class: "ValueError".into(),
            message: message.into(),
        }
    }

    /// A `TypeError` with PHP's message.
    pub fn type_error(message: impl Into<String>) -> PasswordError {
        PasswordError::Throw {
            class: "TypeError".into(),
            message: message.into(),
        }
    }
}

impl std::fmt::Display for PasswordError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PasswordError::Throw { class, message } => write!(f, "{class}: {message}"),
            PasswordError::Rethrow(_) => f.write_str("a PHP exception"),
            PasswordError::Unwinding => f.write_str("PHP is unwinding"),
            PasswordError::Port(why) => f.write_str(why),
        }
    }
}

impl std::error::Error for PasswordError {}

/// `wp_hash_password()`.
pub trait PasswordHasher {
    /// Hashes `password`.
    fn hash(&self, password: &[u8]) -> Result<Value, PasswordError>;
}

/// `wp_check_password()`.
pub trait PasswordChecker {
    /// Whether `password` matches `hash`, as the `check_password` filter last said. `user_id` is passed on to the
    /// filter untouched.
    fn check(&self, password: &[u8], hash: &[u8], user_id: &Value) -> Result<Value, PasswordError>;
}

/// `wp_password_needs_rehash()`.
pub trait RehashPolicy {
    /// Whether `hash` should be replaced by a fresh hash of the same password.
    fn needs_rehash(&self, hash: &[u8], user_id: &Value) -> Result<Value, PasswordError>;
}

/// `wp_generate_password()`.
pub trait PasswordGenerator {
    /// A random password of `length` characters, special characters included when asked. The arguments are PHP
    /// values: WordPress loops while `$i < $length`, tests the flags for truth, and passes all three to its filter.
    fn generate(
        &self,
        length: &Value,
        special: &Value,
        extra: &Value,
    ) -> Result<Value, PasswordError>;
}

/// `wp_rand()`, after its `null` defaults and `(int)` casts.
pub trait RandomNumber {
    /// A random number between `min` and `max`, in either order.
    fn rand(&self, min: i64, max: i64) -> Result<Value, PasswordError>;
}

/// The filters WordPress's password functions apply, with WordPress's arguments in WordPress's order.
pub trait PasswordFilters {
    /// `wp_hash_password_algorithm`.
    fn algorithm(&self, default: Value) -> Result<Value, PasswordError>;
    /// `wp_hash_password_options`, given the filtered algorithm.
    fn options(&self, options: Value, algorithm: &Value) -> Result<Value, PasswordError>;
    /// `check_password`. `check` is a value, not a bool: when `$wp_hasher` answered, WordPress passes on whatever
    /// its `CheckPassword()` returned.
    fn check_password(
        &self,
        check: Value,
        password: &[u8],
        hash: &[u8],
        user_id: &Value,
    ) -> Result<Value, PasswordError>;
    /// `password_needs_rehash`.
    fn needs_rehash(
        &self,
        needs: bool,
        hash: &[u8],
        user_id: &Value,
    ) -> Result<Value, PasswordError>;
    /// `random_password`.
    fn random_password(
        &self,
        password: Vec<u8>,
        length: &Value,
        special: &Value,
        extra: &Value,
    ) -> Result<Value, PasswordError>;
}

/// The `$wp_hasher` global: a plugin's replacement hasher, honoured as WordPress honours it.
///
/// An empty global is "no hasher" (a Special Case), so the defaults need not know about PHP.
pub trait WpHasherGlobal {
    /// Whether `$wp_hasher` is non-empty.
    fn is_set(&self) -> bool;
    /// `$wp_hasher->HashPassword( $trimmed )`, or `None` when no hasher is set.
    fn hash(&self, trimmed: &[u8]) -> Option<Result<Value, PasswordError>>;
    /// `$wp_hasher->CheckPassword( $password, $hash )`, or `None` when no hasher is set.
    fn check(&self, password: &[u8], hash: &[u8]) -> Option<Result<Value, PasswordError>>;
}

/// bcrypt, as PHP's `crypt()` and `password_verify()` compute it.
pub trait Bcrypt {
    /// The `$2y$` hash of `password` with `cost` and `salt`.
    ///
    /// # Errors
    ///
    /// When the implementation can't hash with these arguments.
    fn hash(&self, password: &[u8], cost: u32, salt: &[u8; 16]) -> Result<String, String>;
    /// Whether `password` matches a `$2a$`, `$2b$` or `$2y$` hash. Any other hash never matches.
    fn verify(&self, password: &[u8], hash: &[u8]) -> bool;
}

/// Warnings PHP raises from `password_hash()`, such as its ignored `salt` option.
pub trait Warnings {
    /// Raises an `E_WARNING` with `message`.
    fn warn(&self, message: &str);
}
