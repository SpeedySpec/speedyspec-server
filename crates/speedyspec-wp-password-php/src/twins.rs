//! The PHP twin interfaces of the five pluggable ports, which PHP implements to replace one, and of the site's cost
//! limits.
//!
//! `#[php_port]` declares each interface at the start of every request. Values cross untyped: WordPress's functions
//! take and return any PHP value. Calls into a PHP implementation go through [`crate::adapters`], which keeps the
//! exception a replacement throws so the caller sees it unchanged.

use speedyspec_php_ext::{PortError, Value, php_port};

/// `wp_hash_password()`'s twin.
#[php_port(name = "SpeedySpec\\WpPassword\\PasswordHasher")]
pub trait PasswordHasherTwin {
    fn hash(&self, password: Value) -> Result<Value, PortError>;
}

/// `wp_check_password()`'s twin.
#[php_port(name = "SpeedySpec\\WpPassword\\PasswordChecker")]
pub trait PasswordCheckerTwin {
    fn check(&self, password: Value, hash: Value, user_id: Value) -> Result<Value, PortError>;
}

/// `wp_password_needs_rehash()`'s twin.
#[php_port(name = "SpeedySpec\\WpPassword\\RehashPolicy")]
pub trait RehashPolicyTwin {
    fn needs_rehash(&self, hash: Value, user_id: Value) -> Result<Value, PortError>;
}

/// `wp_generate_password()`'s twin.
#[php_port(name = "SpeedySpec\\WpPassword\\PasswordGenerator")]
pub trait PasswordGeneratorTwin {
    fn generate(&self, length: Value, special: Value, extra: Value) -> Result<Value, PortError>;
}

/// `wp_rand()`'s twin.
#[php_port(name = "SpeedySpec\\WpPassword\\RandomNumber")]
pub trait RandomNumberTwin {
    fn rand(&self, min: i64, max: i64) -> Result<Value, PortError>;
}

/// The site's cost limits' twin. `limits()` returns an array with any of `cost` (bcrypt), `memory_cost` (argon2,
/// KiB) and `time_cost` (argon2), as `password_hash()` names its options; a missing key keeps its default.
#[php_port(name = "SpeedySpec\\WpPassword\\PasswordCostLimits")]
pub trait PasswordCostLimitsTwin {
    fn limits(&self) -> Result<Value, PortError>;
}
