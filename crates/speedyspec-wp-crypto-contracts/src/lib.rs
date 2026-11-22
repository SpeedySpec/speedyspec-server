//! Contracts for WordPress's sodium functions: `wp_fast_hash()`, `wp_verify_fast_hash()`, `wp_trusted_keys()` and
//! `verify_file_signature()`.
//!
//! WordPress's rules (the `$generic$` format, the signature loop, the error data) depend only on these ports, so the
//! cryptography behind them can be replaced. The SpeedySpec runtime implements the crypto ports with libsodium, the
//! same library PHP's ext-sodium links, and the WordPress-facing ports over PHP:
//!
//! - [`GenericHash`] is `sodium_crypto_generichash()` (keyed BLAKE2b);
//! - [`DetachedVerifier`] is `sodium_crypto_sign_verify_detached()` (Ed25519);
//! - [`Base64UrlNoPad`] is `sodium_bin2base64()` with `SODIUM_BASE64_VARIANT_URLSAFE_NO_PADDING`;
//! - [`CryptoLibrary`] is `SODIUM_LIBRARY_VERSION`, which WordPress puts in its error data;
//! - [`Clock`] is `time()`, for the expiry of WordPress.org's first signing key;
//! - [`FileHasher`] is `hash_file( 'sha384', $filename, true )`;
//! - [`Translator`] is `__()` in the default text domain;
//! - [`TrustedKeysFilter`] is the `wp_trusted_keys` filter;
//! - [`Argon2`] is `sodium_crypto_pwhash()` (raw argon2i/argon2id), under WordPress's password hashing;
//! - [`RandomBytes`] is `random_bytes()`, for salts and `wp_rand()`.
//!
//! Messages, hashes and keys are bytes throughout, as PHP strings are.

/// The length of an Ed25519 detached signature (`SODIUM_CRYPTO_SIGN_BYTES`).
pub const SIGN_BYTES: usize = 64;

/// The length of an Ed25519 public key (`SODIUM_CRYPTO_SIGN_PUBLICKEYBYTES`).
pub const SIGN_PUBLICKEY_BYTES: usize = 32;

/// Keyed BLAKE2b, as `sodium_crypto_generichash()`.
pub trait GenericHash {
    /// Hashes `message` with `key` into `out_len` bytes.
    fn hash(&self, message: &[u8], key: &[u8], out_len: usize) -> Vec<u8>;
}

/// Ed25519 detached signature verification, as `sodium_crypto_sign_verify_detached()`.
pub trait DetachedVerifier {
    /// Whether `signature` is `public_key`'s signature of `message`.
    fn verify(
        &self,
        signature: &[u8; SIGN_BYTES],
        message: &[u8],
        public_key: &[u8; SIGN_PUBLICKEY_BYTES],
    ) -> bool;
}

/// URL-safe base64 without padding, as `sodium_bin2base64()` with `SODIUM_BASE64_VARIANT_URLSAFE_NO_PADDING`.
pub trait Base64UrlNoPad {
    /// Encodes `bytes`.
    fn encode(&self, bytes: &[u8]) -> String;
}

/// The cryptography library itself.
pub trait CryptoLibrary {
    /// The library's version, as `SODIUM_LIBRARY_VERSION` reports it.
    fn version(&self) -> String;
}

/// The current time, as `time()`.
pub trait Clock {
    /// Seconds since the Unix epoch.
    fn now(&self) -> i64;
}

/// A file's sha384, as `hash_file( 'sha384', $filename, true )`.
///
/// The filename is PHP's: an adapter over PHP opens it as `hash_file()` does (stream wrappers included) and raises what
/// `hash_file()` raises (its warning for an unreadable file, its `ValueError` for a NUL byte).
pub trait FileHasher {
    /// Why hashing failed where `hash_file()` throws.
    type Error;

    /// The raw digest, `None` where `hash_file()` returns `false`.
    fn sha384(&self, filename: &[u8]) -> Result<Option<[u8; 48]>, Self::Error>;
}

/// WordPress's translation of its own strings, as `__()` in the `default` domain.
///
/// Translating runs the `gettext` filters, whose callbacks may throw; WordPress lets that exception propagate.
pub trait Translator {
    /// Why translating failed (a filter callback's exception).
    type Error;

    /// The translation of `text`, or `text` itself.
    fn translate(&self, text: &str) -> Result<String, Self::Error>;
}

/// The `wp_trusted_keys` filter.
///
/// It answers the filtered value as the filter returned it: WordPress returns that value from `wp_trusted_keys()` and
/// in its error data untouched, and reads its elements as keys only when its signature loop reaches them.
pub trait TrustedKeysFilter {
    /// The filtered value, as the adapter holds it.
    type Value;
    /// Why filtering failed (a callback's exception).
    type Error;

    /// Filters WordPress's default keys (base64 text).
    fn apply(&self, defaults: Vec<Vec<u8>>) -> Result<Self::Value, Self::Error>;
}

/// Which argon2 a password hash uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Argon2Variant {
    /// `$argon2i$`: data-independent memory access.
    Argon2i,
    /// `$argon2id$`: the hybrid, and SpeedySpec's default password hash.
    Argon2id,
}

/// Raw argon2 (version 19, one lane), as `sodium_crypto_pwhash()`.
///
/// Password hashing builds its `$argon2…$` strings on top of this, so the string format is WordPress's and PHP's rule
/// and the key derivation stays replaceable.
pub trait Argon2 {
    /// Derives `out_len` bytes from `password` and `salt` with `time_cost` passes over `memory_kib` KiB.
    ///
    /// # Errors
    ///
    /// When the parameters are outside what the implementation computes (libsodium refuses argon2i below three
    /// passes, and memory below its minimum).
    fn derive(
        &self,
        variant: Argon2Variant,
        password: &[u8],
        salt: &[u8; 16],
        time_cost: u32,
        memory_kib: u32,
        out_len: usize,
    ) -> Result<Vec<u8>, String>;
}

/// A cryptographically secure source of random bytes, as `random_bytes()`.
pub trait RandomBytes {
    /// Fills `out` with random bytes.
    fn fill(&self, out: &mut [u8]);
}
