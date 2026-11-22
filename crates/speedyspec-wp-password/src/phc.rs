//! argon2's hash strings, in the PHC format PHP writes: `$argon2id$v=19$m=65536,t=4,p=1$<salt>$<digest>`.
//!
//! The salt and digest are standard base64 without padding. SpeedySpec builds and reads the string itself over raw
//! argon2 ([`Argon2`](speedyspec_wp_crypto_contracts::Argon2)), so a given salt gives the same string as PHP.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD_NO_PAD;
use speedyspec_wp_crypto_contracts::Argon2Variant;

/// argon2's version 1.3, the one libsodium and PHP write.
pub const ARGON2_VERSION: u32 = 19;

/// A parsed argon2 hash string.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Phc {
    pub variant: Argon2Variant,
    /// 19, or 16 when the string has no `v=` (argon2 1.0).
    pub version: u32,
    pub memory_kib: u32,
    pub time_cost: u32,
    pub threads: u32,
    pub salt: Vec<u8>,
    pub digest: Vec<u8>,
}

/// The hash string of `digest`, derived with `variant`, `time_cost`, `memory_kib` KiB and one lane from `salt`.
pub fn argon2_phc(
    variant: Argon2Variant,
    time_cost: u32,
    memory_kib: u32,
    salt: &[u8; 16],
    digest: &[u8],
) -> String {
    format!(
        "${}$v={ARGON2_VERSION}$m={memory_kib},t={time_cost},p=1${}${}",
        variant_name(variant),
        STANDARD_NO_PAD.encode(salt),
        STANDARD_NO_PAD.encode(digest)
    )
}

/// The name a hash string starts with.
pub fn variant_name(variant: Argon2Variant) -> &'static str {
    match variant {
        Argon2Variant::Argon2i => "argon2i",
        Argon2Variant::Argon2id => "argon2id",
    }
}

/// `hash` read as an argon2 hash string, or `None` when it isn't one.
pub fn parse_phc(hash: &[u8]) -> Option<Phc> {
    let text = std::str::from_utf8(hash).ok()?;
    let mut parts = text.split('$');
    if !parts.next()?.is_empty() {
        return None;
    }
    let variant = match parts.next()? {
        "argon2i" => Argon2Variant::Argon2i,
        "argon2id" => Argon2Variant::Argon2id,
        _ => return None,
    };
    let mut next = parts.next()?;
    let mut version = 16;
    if let Some(v) = next.strip_prefix("v=") {
        version = v.parse().ok()?;
        next = parts.next()?;
    }
    let (mut memory_kib, mut time_cost, mut threads) = (None, None, None);
    for param in next.split(',') {
        let (name, value) = param.split_once('=')?;
        let value: u32 = value.parse().ok()?;
        match name {
            "m" => memory_kib = Some(value),
            "t" => time_cost = Some(value),
            "p" => threads = Some(value),
            _ => return None,
        }
    }
    let salt = STANDARD_NO_PAD.decode(parts.next()?).ok()?;
    let digest = STANDARD_NO_PAD.decode(parts.next()?).ok()?;
    if parts.next().is_some() || salt.is_empty() || digest.is_empty() {
        return None;
    }
    Some(Phc {
        variant,
        version,
        memory_kib: memory_kib?,
        time_cost: time_cost?,
        threads: threads?,
        salt,
        digest,
    })
}
