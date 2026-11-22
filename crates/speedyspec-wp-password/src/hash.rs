//! `wp_hash_password()`: a Strategy per algorithm, chosen by the filtered algorithm.

use std::rc::Rc;

use speedyspec_hooks::Value;
use speedyspec_wp_crypto_contracts::{Argon2, Argon2Variant, RandomBytes};
use speedyspec_wp_password_contracts::{
    Bcrypt, PasswordError, PasswordFilters, PasswordHasher, Warnings, WpHasherGlobal,
};

use crate::algorithm::{
    Algorithm, HashOptions, algorithm_type_error, is_wp_bcrypt, resolve_algorithm, resolve_options,
};
use crate::phc::argon2_phc;
use crate::prehash::{php_trim, prehash};

/// The algorithm SpeedySpec hashes new passwords with, as `wp_hash_password_algorithm`'s default. WordPress's is
/// `PASSWORD_BCRYPT`; SpeedySpec's is `PASSWORD_ARGON2ID`, through libsodium.
pub const DEFAULT_ALGORITHM: &str = "argon2id";

/// The longest password WordPress hashes or checks, in bytes.
pub const MAX_PASSWORD_LEN: usize = 4096;

/// What `password_hash()` answers when its algorithm fails after validation.
const HASH_FAILURE: &str = "Unexpected failure hashing password";

/// bcrypt's default cost (PHP 8.4+).
pub const BCRYPT_DEFAULT_COST: i64 = 12;

/// argon2's defaults, as PHP's `PASSWORD_ARGON2_DEFAULT_*`.
pub const ARGON2_DEFAULT_MEMORY_KIB: i64 = 65536;
pub const ARGON2_DEFAULT_TIME_COST: i64 = 4;
pub const ARGON2_DEFAULT_THREADS: i64 = 1;

/// The ports WordPress's password functions reach outside themselves through.
pub struct PasswordPorts {
    pub filters: Rc<dyn PasswordFilters>,
    pub hasher_global: Rc<dyn WpHasherGlobal>,
    pub bcrypt: Rc<dyn Bcrypt>,
    pub argon2: Rc<dyn Argon2>,
    pub random: Rc<dyn RandomBytes>,
    pub warnings: Rc<dyn Warnings>,
}

/// One way of hashing a password, as `password_hash()` runs it.
pub trait HashAlgorithm {
    /// Hashes `password` with `options`, validating the options this algorithm reads.
    fn hash(&self, password: &[u8], options: &HashOptions) -> Result<String, PasswordError>;
}

/// `password_hash()`'s bcrypt.
pub struct BcryptStrategy<'a>(pub &'a PasswordPorts);

impl HashAlgorithm for BcryptStrategy<'_> {
    fn hash(&self, password: &[u8], options: &HashOptions) -> Result<String, PasswordError> {
        if password.contains(&0) {
            return Err(PasswordError::value_error(
                "Bcrypt password must not contain null character",
            ));
        }
        let cost = options.cost.unwrap_or(BCRYPT_DEFAULT_COST);
        if !(4..=31).contains(&cost) {
            return Err(PasswordError::value_error(format!(
                "Invalid bcrypt cost parameter specified: {cost}"
            )));
        }
        if options.salt {
            self.0.warnings.warn(
                "password_hash(): The \"salt\" option has been ignored, since providing a custom salt is no longer supported",
            )?;
        }
        let mut salt = [0u8; 16];
        self.0.random.fill(&mut salt);
        self.0
            .bcrypt
            .hash(password, cost as u32, &salt)
            .map_err(|_| PasswordError::value_error(HASH_FAILURE))
    }
}

/// WordPress 6.8's bcrypt: the pre-hash of the (trimmed) password, bcrypt, and a `$wp` prefix.
pub struct WpBcryptStrategy<'a>(pub &'a PasswordPorts);

impl HashAlgorithm for WpBcryptStrategy<'_> {
    fn hash(&self, password: &[u8], options: &HashOptions) -> Result<String, PasswordError> {
        let inner = BcryptStrategy(self.0).hash(prehash(password).as_bytes(), options)?;
        Ok(format!("$wp{inner}"))
    }
}

/// `password_hash()`'s argon2i or argon2id, over libsodium: one lane, and argon2i from three passes.
pub struct Argon2Strategy<'a> {
    pub ports: &'a PasswordPorts,
    pub variant: Argon2Variant,
}

impl HashAlgorithm for Argon2Strategy<'_> {
    fn hash(&self, password: &[u8], options: &HashOptions) -> Result<String, PasswordError> {
        let memory = options.memory_cost.unwrap_or(ARGON2_DEFAULT_MEMORY_KIB);
        if !(8..=i64::from(u32::MAX)).contains(&memory) {
            return Err(PasswordError::value_error(
                "Memory cost is outside of allowed memory range",
            ));
        }
        let time = options.time_cost.unwrap_or(ARGON2_DEFAULT_TIME_COST);
        if !(1..=i64::from(u32::MAX)).contains(&time) {
            return Err(PasswordError::value_error(
                "Time cost is outside of allowed time range",
            ));
        }
        let threads = options.threads.unwrap_or(ARGON2_DEFAULT_THREADS);
        if !(1..=0xFF_FFFF).contains(&threads) {
            return Err(PasswordError::value_error("Invalid number of threads"));
        }
        if threads != 1 {
            return Err(PasswordError::value_error(
                "A thread value other than 1 is not supported by this implementation",
            ));
        }
        if self.variant == Argon2Variant::Argon2i && time < 3 {
            return Err(PasswordError::value_error(
                "Time cost is outside of allowed time range",
            ));
        }
        let mut salt = [0u8; 16];
        self.ports.random.fill(&mut salt);
        let digest = self
            .ports
            .argon2
            .derive(
                self.variant,
                password,
                &salt,
                time as u32,
                memory as u32,
                32,
            )
            .map_err(|_| PasswordError::value_error(HASH_FAILURE))?;
        Ok(argon2_phc(
            self.variant,
            time as u32,
            memory as u32,
            &salt,
            &digest,
        ))
    }
}

/// WordPress's `wp_hash_password()`, with argon2id as the default and argon2 hashing the trimmed password.
pub struct DefaultPasswordHasher {
    ports: Rc<PasswordPorts>,
}

impl DefaultPasswordHasher {
    pub fn new(ports: Rc<PasswordPorts>) -> DefaultPasswordHasher {
        DefaultPasswordHasher { ports }
    }
}

impl PasswordHasher for DefaultPasswordHasher {
    fn hash(&self, password: &[u8]) -> Result<Value, PasswordError> {
        let ports = &*self.ports;
        if let Some(result) = ports.hasher_global.hash(php_trim(password)) {
            return result;
        }
        if password.len() > MAX_PASSWORD_LEN {
            return Ok(Value::Str("*".into()));
        }
        let algorithm = ports
            .filters
            .algorithm(Value::Str(DEFAULT_ALGORITHM.into()))?;
        let options = ports
            .filters
            .options(Value::Array(Vec::new()), &algorithm)?;
        let hash = if is_wp_bcrypt(&algorithm) {
            let options = resolve_options(&options, "password_hash")?;
            WpBcryptStrategy(ports).hash(php_trim(password), &options)?
        } else {
            if let Some(err) = algorithm_type_error(&algorithm, "password_hash") {
                return Err(err);
            }
            let options = resolve_options(&options, "password_hash")?;
            match resolve_algorithm(&algorithm, "password_hash")? {
                Algorithm::Bcrypt => BcryptStrategy(ports).hash(password, &options)?,
                Algorithm::Argon2i => Argon2Strategy {
                    ports,
                    variant: Argon2Variant::Argon2i,
                }
                .hash(php_trim(password), &options)?,
                Algorithm::Argon2id => Argon2Strategy {
                    ports,
                    variant: Argon2Variant::Argon2id,
                }
                .hash(php_trim(password), &options)?,
            }
        };
        Ok(Value::Str(hash))
    }
}
