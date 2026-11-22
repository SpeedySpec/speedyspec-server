//! Application passwords' hashing, checking and display, as `WP_Application_Passwords` does them.
//!
//! The class itself stays WordPress's (its storage belongs to a users port); these are its three password rules.

use speedyspec_hooks::Value;
use speedyspec_wp_crypto::{FAST_HASH_PREFIX, wp_fast_hash, wp_verify_fast_hash};
use speedyspec_wp_crypto_contracts::{Base64UrlNoPad, GenericHash};
use speedyspec_wp_password_contracts::{PasswordChecker, PasswordError};

use crate::prehash::php_trim;
use crate::random::php_truthy;

/// `WP_Application_Passwords::hash_password()`: the fast hash.
pub fn application_password_hash(
    hash: &dyn GenericHash,
    base64: &dyn Base64UrlNoPad,
    password: &[u8],
) -> String {
    wp_fast_hash(hash, base64, password)
}

/// `WP_Application_Passwords::check_password()`: a `$generic$` hash is a fast hash; anything else was written by
/// `wp_hash_password()` and goes to the bound `wp_check_password()`. The class declares a `bool` return, so the
/// checker's answer is coerced to one.
pub fn application_password_check(
    checker: &dyn PasswordChecker,
    hash: &dyn GenericHash,
    base64: &dyn Base64UrlNoPad,
    password: &[u8],
    stored: &[u8],
) -> Result<bool, PasswordError> {
    if stored.starts_with(FAST_HASH_PREFIX.as_bytes()) {
        return Ok(wp_verify_fast_hash(hash, base64, password, stored));
    }
    Ok(php_truthy(&checker.check(
        password,
        stored,
        &Value::Str(String::new()),
    )?))
}

/// `WP_Application_Passwords::chunk_password()`: `trim( chunk_split( $raw, 4, ' ' ) )`, for display.
pub fn chunk_password(raw: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(raw.len() + raw.len() / 4 + 1);
    for chunk in raw.chunks(4) {
        out.extend_from_slice(chunk);
        out.push(b' ');
    }
    php_trim(&out).to_vec()
}
