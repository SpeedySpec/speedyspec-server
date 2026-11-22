//! WordPress's password rules, independent of the cryptography and of PHP.
//!
//! - [`DefaultPasswordHasher`] is `wp_hash_password()`: a Strategy per algorithm ([`WpBcryptStrategy`],
//!   [`BcryptStrategy`], [`Argon2Strategy`]) chosen by the filtered algorithm, argon2id by default.
//!
//! What the rules need from outside comes in through the ports of `speedyspec-wp-password-contracts` and
//! `speedyspec-wp-crypto-contracts`, gathered in [`PasswordPorts`]. Two answers differ from WordPress on purpose:
//! argon2id is the default algorithm, and argon2 hashes the trimmed password, as WordPress's bcrypt does.

mod algorithm;
mod hash;
mod phc;
mod prehash;

pub use algorithm::{
    Algorithm, HashOptions, algorithm_type_error, is_wp_bcrypt, php_long, php_type_name,
    resolve_algorithm, resolve_options,
};
pub use hash::{
    ARGON2_DEFAULT_MEMORY_KIB, ARGON2_DEFAULT_THREADS, ARGON2_DEFAULT_TIME_COST, Argon2Strategy,
    BCRYPT_DEFAULT_COST, BcryptStrategy, DEFAULT_ALGORITHM, DefaultPasswordHasher, HashAlgorithm,
    MAX_PASSWORD_LEN, PasswordPorts, WpBcryptStrategy,
};
pub use phc::{ARGON2_VERSION, Phc, argon2_phc, parse_phc, variant_name};
pub use prehash::{PREHASH_KEY, php_trim, prehash};
