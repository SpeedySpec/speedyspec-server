//! bcrypt for the password port, over the `bcrypt` crate.
//!
//! libsodium has no bcrypt, and SpeedySpec still needs it: WordPress 6.8+ stores `$wp$2y$` hashes and older sites
//! plain `$2y$` ones. [`BcryptCrate`] matches PHP byte for byte. Two of PHP's rules shape it:
//!
//! - PHP's crypt_blowfish reads a password as a C string, so it stops at the first NUL byte; the crate would read
//!   on. The adapter cuts the password at its first NUL before hashing or verifying.
//! - `$2x$` hashes carry crypt_blowfish's sign-extension bug for 8-bit characters, which the crate doesn't reproduce.
//!   SpeedySpec doesn't verify them (WordPress never writes them); only `$2a$`, `$2b$` and `$2y$` verify.

use bcrypt::Version;
use speedyspec_wp_password_contracts::Bcrypt;

/// bcrypt over the `bcrypt` crate.
#[derive(Clone, Copy, Debug, Default)]
pub struct BcryptCrate;

/// `password` up to its first NUL byte, as crypt_blowfish reads it.
fn c_string(password: &[u8]) -> &[u8] {
    match password.iter().position(|b| *b == 0) {
        Some(end) => &password[..end],
        None => password,
    }
}

impl Bcrypt for BcryptCrate {
    fn hash(&self, password: &[u8], cost: u32, salt: &[u8; 16]) -> Result<String, String> {
        bcrypt::hash_with_salt(c_string(password), cost, *salt)
            .map(|parts| parts.format_for_version(Version::TwoY))
            .map_err(|err| err.to_string())
    }

    fn verify(&self, password: &[u8], hash: &[u8]) -> bool {
        let Ok(hash) = std::str::from_utf8(hash) else {
            return false;
        };
        if !(hash.starts_with("$2y$") || hash.starts_with("$2a$") || hash.starts_with("$2b$")) {
            return false;
        }
        bcrypt::verify(c_string(password), hash).unwrap_or(false)
    }
}
