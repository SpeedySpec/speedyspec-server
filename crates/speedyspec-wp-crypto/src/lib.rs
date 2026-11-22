//! WordPress's sodium functions' rules, independent of the cryptography behind them.
//!
//! - [`wp_fast_hash`] and [`wp_verify_fast_hash`]: keyed BLAKE2b behind WordPress's `$generic$` prefix. A hash without
//!   the prefix never verifies: SpeedySpec does not carry phpass, so the pre-6.8 phpass fallback is gone on purpose.
//! - [`default_trusted_keys`]: `wp_trusted_keys()` before its filter.
//! - [`check_signatures`]: `verify_file_signature()`'s loop over signatures and keys, with its skip counts.
//! - [`messages`]: its translated error messages.
//!
//! The crypto comes in through the ports of `speedyspec-wp-crypto-contracts`; this crate owns only the rules.

mod base64;
mod basename;
mod fast_hash;
mod signature;
mod trusted_keys;

pub use base64::php_base64_decode;
pub use basename::wp_basename;
pub use fast_hash::{
    FAST_HASH_KEY, FAST_HASH_LEN, FAST_HASH_PREFIX, wp_fast_hash, wp_verify_fast_hash,
};
pub use signature::{
    SignatureMessages, SignatureOutcome, check_signatures, hex, messages, sha384_file,
};
pub use trusted_keys::{WPORG_KEY_1, WPORG_KEY_1_UNTIL, default_trusted_keys};
