//! `md5( serialize( $args ) )` in Rust, for arguments that hold no PHP object.

use md5::{Digest, Md5};
use speedyspec_php_serialize::{from_hook_value, serialize};
use speedyspec_wp_cron_contracts::{ArgsKeyer, CronError, EventKey};
use speedyspec_wp_hooks_contracts::HookValue;

/// The bytes' MD5 as 32 lowercase hexadecimal characters.
pub fn md5_hex(bytes: &[u8]) -> String {
    Md5::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Keys arguments through `speedyspec-php-serialize`; a PHP object in them is an error (the PHP bridge's keyer
/// serializes those through PHP).
pub struct RustArgsKeyer;

impl ArgsKeyer for RustArgsKeyer {
    fn key(&self, args: &HookValue) -> Result<EventKey, CronError> {
        let value = from_hook_value(&args.read()).map_err(|e| {
            CronError::new(format!(
                "cron arguments cannot be serialized in Rust: {e:?}"
            ))
        })?;
        Ok(EventKey::found(md5_hex(&serialize(&value))))
    }
}
