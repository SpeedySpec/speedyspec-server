//! WordPress's password rules, independent of the cryptography and of PHP.
//!
//! - [`DefaultPasswordHasher`] is `wp_hash_password()`: a Strategy per algorithm ([`WpBcryptStrategy`],
//!   [`BcryptStrategy`], [`Argon2Strategy`]) chosen by the filtered algorithm, argon2id by default.
//! - [`DefaultPasswordChecker`] is `wp_check_password()`: a Chain of Responsibility over hash formats
//!   ([`default_chain`]), in WordPress's order.
//! - [`DefaultRehashPolicy`] is `wp_password_needs_rehash()`, over [`password_needs_rehash`].
//! - [`DefaultRandomNumber`] is `wp_rand()` ([`uniform`] is `random_int()`), and [`DefaultPasswordGenerator`]
//!   `wp_generate_password()`, drawing through whatever `wp_rand()` is bound.
//! - [`application_password_hash`], [`application_password_check`] and [`chunk_password`] are
//!   `WP_Application_Passwords`' password rules.
//!
//! What the rules need from outside comes in through the ports of `speedyspec-wp-password-contracts` and
//! `speedyspec-wp-crypto-contracts`, gathered in [`PasswordPorts`]. Answers that differ from WordPress on purpose:
//! argon2id is the default algorithm; argon2 hashes the trimmed password, as WordPress's bcrypt does; raw MD5, phpass,
//! `$2x$` and `crypt()`-only hashes never verify.

mod algorithm;
mod application;
mod check;
mod hash;
mod phc;
mod prehash;
mod random;
mod rehash;

pub use algorithm::{
    Algorithm, HashOptions, algorithm_type_error, is_wp_bcrypt, php_long, php_type_name,
    resolve_algorithm, resolve_options,
};
pub use application::{application_password_check, application_password_hash, chunk_password};
pub use check::{
    Argon2Format, BcryptFormat, CheckHandler, DefaultPasswordChecker, Handled, HasherOverride,
    Md5Rejected, PhpassRejected, TooLong, Unrecognized, WpPrefixed, default_chain, verify_argon2,
    verify_bcrypt,
};
pub use hash::{
    ARGON2_DEFAULT_MEMORY_KIB, ARGON2_DEFAULT_THREADS, ARGON2_DEFAULT_TIME_COST, Argon2Strategy,
    BCRYPT_DEFAULT_COST, BcryptStrategy, DEFAULT_ALGORITHM, DefaultPasswordHasher, FixedCostLimits,
    HashAlgorithm, MAX_PASSWORD_LEN, PasswordPorts, WpBcryptStrategy,
};
pub use phc::{ARGON2_VERSION, Phc, argon2_phc, parse_phc, variant_name};
pub use prehash::{PREHASH_KEY, php_trim, prehash};
pub use random::{
    CHARS, DefaultPasswordGenerator, DefaultRandomNumber, EXTRA_SPECIAL, RandomNumberResolver,
    SPECIAL, php_truthy, uniform,
};
pub use rehash::{DefaultRehashPolicy, password_needs_rehash};
