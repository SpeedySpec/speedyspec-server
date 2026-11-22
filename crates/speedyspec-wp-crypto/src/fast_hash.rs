//! `wp_fast_hash()` and `wp_verify_fast_hash()`.

use speedyspec_wp_crypto_contracts::{Base64UrlNoPad, GenericHash};

/// What every fast hash starts with.
pub const FAST_HASH_PREFIX: &str = "$generic$";

/// The BLAKE2b key WordPress 6.8 introduced, so its hashes never collide with other uses of the same input.
pub const FAST_HASH_KEY: &[u8] = b"wp_fast_hash_6.8+";

/// The digest length in bytes.
pub const FAST_HASH_LEN: usize = 30;

/// Hashes a high-entropy message (a security key, an application password) as `wp_fast_hash()` does.
pub fn wp_fast_hash(hash: &dyn GenericHash, base64: &dyn Base64UrlNoPad, message: &[u8]) -> String {
    let digest = hash.hash(message, FAST_HASH_KEY, FAST_HASH_LEN);
    format!("{FAST_HASH_PREFIX}{}", base64.encode(&digest))
}

/// Checks a message against a fast hash, as `wp_verify_fast_hash()` does, but without its phpass fallback: a hash
/// that is not a fast hash never verifies.
pub fn wp_verify_fast_hash(
    hash: &dyn GenericHash,
    base64: &dyn Base64UrlNoPad,
    message: &[u8],
    expected: &[u8],
) -> bool {
    if !expected.starts_with(FAST_HASH_PREFIX.as_bytes()) {
        return false;
    }
    constant_time_eq(expected, wp_fast_hash(hash, base64, message).as_bytes())
}

/// `hash_equals()`: the time taken depends only on the lengths.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}
