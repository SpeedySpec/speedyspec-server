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
//! - [`Translator`] is `__()` in the default text domain;
//! - [`TrustedKeysFilter`] is the `wp_trusted_keys` filter.
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
    fn verify(&self, signature: &[u8; SIGN_BYTES], message: &[u8], public_key: &[u8; SIGN_PUBLICKEY_BYTES]) -> bool;
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

/// WordPress's translation of its own strings, as `__()` in the `default` domain.
pub trait Translator {
    /// The translation of `text`, or `text` itself.
    fn translate(&self, text: &str) -> String;
}

/// The trusted keys after the `wp_trusted_keys` filter.
///
/// `keys` are the strings the signature check decodes, in order. `raw` is the filter's return value exactly as the
/// filter returned it, held by the adapter: WordPress returns it untouched in its error data, and the rules never look
/// inside it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FilteredKeys<H> {
    /// The keys as base64 text, in the filtered order.
    pub keys: Vec<Vec<u8>>,
    /// The filtered value as the filter returned it.
    pub raw: H,
}

/// The `wp_trusted_keys` filter.
pub trait TrustedKeysFilter {
    /// The adapter's handle on the filtered value.
    type Raw;

    /// Filters WordPress's default keys (base64 text).
    fn apply(&self, defaults: Vec<Vec<u8>>) -> FilteredKeys<Self::Raw>;
}
