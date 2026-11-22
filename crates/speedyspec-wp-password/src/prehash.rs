//! WordPress 6.8's bcrypt pre-hash, and PHP's `trim()`.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use hmac::{Hmac, Mac};
use sha2::Sha384;

/// The HMAC key WordPress uses, so the pre-hash never collides with another use of SHA-384.
pub const PREHASH_KEY: &[u8] = b"wp-sha384";

/// `base64_encode( hash_hmac( 'sha384', $password, 'wp-sha384', true ) )`.
///
/// bcrypt reads only 72 bytes; the pre-hash keeps the entropy of longer passwords, and its 64 base64 characters
/// never contain the NUL byte bcrypt would stop at.
pub fn prehash(password: &[u8]) -> String {
    let mut mac =
        Hmac::<Sha384>::new_from_slice(PREHASH_KEY).expect("HMAC takes keys of any length");
    mac.update(password);
    STANDARD.encode(mac.finalize().into_bytes())
}

/// PHP's `trim()` with its default characters: space, `\t`, `\n`, `\r`, `\0` and `\x0B`.
pub fn php_trim(text: &[u8]) -> &[u8] {
    let blank = |b: &u8| matches!(b, b' ' | b'\t' | b'\n' | b'\r' | b'\0' | b'\x0B');
    let start = text.iter().position(|b| !blank(b)).unwrap_or(text.len());
    let end = text
        .iter()
        .rposition(|b| !blank(b))
        .map_or(start, |i| i + 1);
    &text[start..end]
}
