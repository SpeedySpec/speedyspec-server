//! `wp_check_password()`: a Chain of Responsibility over hash formats, in WordPress's `if/elseif` order.
//!
//! Order is part of the semantics: a 32-character hash is rejected before `$wp_hasher` is asked, and `$wp_hasher`
//! answers before the length and format rules. Two links differ from WordPress on purpose: hashes of 32 characters
//! or fewer (raw MD5) and phpass `$P$` hashes never verify. A third is a hardening: bcrypt and argon2 hashes whose
//! costs exceed the site's [`CostLimits`] never verify, and are never computed.

use std::rc::Rc;

use speedyspec_hooks::Value;
use speedyspec_wp_crypto_contracts::{Argon2, Argon2Variant};
use speedyspec_wp_password_contracts::{Bcrypt, CostLimits, PasswordChecker, PasswordError};
use subtle::ConstantTimeEq;

use crate::hash::{MAX_PASSWORD_LEN, PasswordPorts};
use crate::phc::{ARGON2_VERSION, parse_phc};
use crate::prehash::prehash;

/// What a link of the chain decided.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Handled {
    /// The check's answer, before the `check_password` filter.
    Answer(bool),
    /// Ask `$wp_hasher->CheckPassword()`.
    Delegate,
}

/// One link: it answers for the hashes it recognises, or passes them on (`None`).
pub trait CheckHandler {
    fn handle(&self, password: &[u8], hash: &[u8]) -> Option<Handled>;
}

/// Hashes of 32 characters or fewer: WordPress compares them with raw MD5; SpeedySpec never verifies them.
pub struct Md5Rejected;

impl CheckHandler for Md5Rejected {
    fn handle(&self, _password: &[u8], hash: &[u8]) -> Option<Handled> {
        (hash.len() <= 32).then_some(Handled::Answer(false))
    }
}

/// A plugin's `$wp_hasher` answers everything after the length rule.
pub struct HasherOverride(pub bool);

impl CheckHandler for HasherOverride {
    fn handle(&self, _password: &[u8], _hash: &[u8]) -> Option<Handled> {
        self.0.then_some(Handled::Delegate)
    }
}

/// Passwords longer than WordPress supports.
pub struct TooLong;

impl CheckHandler for TooLong {
    fn handle(&self, password: &[u8], _hash: &[u8]) -> Option<Handled> {
        (password.len() > MAX_PASSWORD_LEN).then_some(Handled::Answer(false))
    }
}

/// WordPress 6.8's `$wp` hashes: the untrimmed password's pre-hash against the rest of the hash.
pub struct WpPrefixed {
    pub bcrypt: Rc<dyn Bcrypt>,
    pub argon2: Rc<dyn Argon2>,
    pub limits: CostLimits,
}

impl CheckHandler for WpPrefixed {
    fn handle(&self, password: &[u8], hash: &[u8]) -> Option<Handled> {
        let rest = hash.strip_prefix(b"$wp")?;
        Some(Handled::Answer(verify_known(
            &*self.bcrypt,
            &*self.argon2,
            &self.limits,
            prehash(password).as_bytes(),
            rest,
        )))
    }
}

/// phpass `$P$` hashes: WordPress checks them with phpass; SpeedySpec carries no phpass.
pub struct PhpassRejected;

impl CheckHandler for PhpassRejected {
    fn handle(&self, _password: &[u8], hash: &[u8]) -> Option<Handled> {
        hash.starts_with(b"$P$").then_some(Handled::Answer(false))
    }
}

/// `$2a$`, `$2b$` and `$2y$` bcrypt, up to the site's cost limit.
pub struct BcryptFormat(pub Rc<dyn Bcrypt>, pub CostLimits);

impl CheckHandler for BcryptFormat {
    fn handle(&self, password: &[u8], hash: &[u8]) -> Option<Handled> {
        is_bcrypt(hash).then(|| Handled::Answer(verify_bcrypt(&*self.0, &self.1, password, hash)))
    }
}

/// `$argon2i$` and `$argon2id$`, up to the site's cost limits.
pub struct Argon2Format(pub Rc<dyn Argon2>, pub CostLimits);

impl CheckHandler for Argon2Format {
    fn handle(&self, password: &[u8], hash: &[u8]) -> Option<Handled> {
        is_argon2(hash).then(|| Handled::Answer(verify_argon2(&*self.0, &self.1, password, hash)))
    }
}

/// Everything else: `$2x$`, `$H$`, `crypt()`-only formats and garbage.
pub struct Unrecognized;

impl CheckHandler for Unrecognized {
    fn handle(&self, _password: &[u8], _hash: &[u8]) -> Option<Handled> {
        Some(Handled::Answer(false))
    }
}

fn is_bcrypt(hash: &[u8]) -> bool {
    hash.starts_with(b"$2y$") || hash.starts_with(b"$2a$") || hash.starts_with(b"$2b$")
}

fn is_argon2(hash: &[u8]) -> bool {
    hash.starts_with(b"$argon2i$") || hash.starts_with(b"$argon2id$")
}

/// `password_verify()` for the formats SpeedySpec supports.
fn verify_known(
    bcrypt: &dyn Bcrypt,
    argon2: &dyn Argon2,
    limits: &CostLimits,
    password: &[u8],
    hash: &[u8],
) -> bool {
    if is_bcrypt(hash) {
        verify_bcrypt(bcrypt, limits, password, hash)
    } else if is_argon2(hash) {
        verify_argon2(argon2, limits, password, hash)
    } else {
        false
    }
}

/// A bcrypt hash's cost (`$2y$NN$`), when it reads as a number.
fn bcrypt_cost(hash: &[u8]) -> Option<u32> {
    let rest = hash.get(4..)?;
    let end = rest.iter().position(|b| *b == b'$')?;
    std::str::from_utf8(&rest[..end]).ok()?.parse().ok()
}

/// Whether `password` matches a bcrypt hash. A hash whose cost is above `limits` never verifies, and isn't computed.
pub fn verify_bcrypt(
    bcrypt: &dyn Bcrypt,
    limits: &CostLimits,
    password: &[u8],
    hash: &[u8],
) -> bool {
    if bcrypt_cost(hash).is_some_and(|cost| cost > limits.bcrypt_cost) {
        return false;
    }
    bcrypt.verify(password, hash)
}

/// Whether `password` matches an argon2 hash string, comparing digests in constant time.
///
/// Hashes libsodium can't recompute never verify: more than one lane, argon2 1.0 (no `v=19`), argon2i under three
/// passes, salts other than 16 bytes. Nor do hashes whose memory or time cost is above `limits`.
pub fn verify_argon2(
    argon2: &dyn Argon2,
    limits: &CostLimits,
    password: &[u8],
    hash: &[u8],
) -> bool {
    let Some(phc) = parse_phc(hash) else {
        return false;
    };
    if phc.memory_kib > limits.argon2_memory_kib || phc.time_cost > limits.argon2_time_cost {
        return false;
    }
    if phc.version != ARGON2_VERSION
        || phc.threads != 1
        || (phc.variant == Argon2Variant::Argon2i && phc.time_cost < 3)
    {
        return false;
    }
    let Ok(salt) = <[u8; 16]>::try_from(phc.salt.as_slice()) else {
        return false;
    };
    match argon2.derive(
        phc.variant,
        password,
        &salt,
        phc.time_cost,
        phc.memory_kib,
        phc.digest.len(),
    ) {
        Ok(digest) => digest.ct_eq(&phc.digest).into(),
        Err(_) => false,
    }
}

/// WordPress's chain, in its order, verifying up to `limits`.
pub fn default_chain(
    bcrypt: Rc<dyn Bcrypt>,
    argon2: Rc<dyn Argon2>,
    hasher_set: bool,
    limits: CostLimits,
) -> Vec<Box<dyn CheckHandler>> {
    vec![
        Box::new(Md5Rejected),
        Box::new(HasherOverride(hasher_set)),
        Box::new(TooLong),
        Box::new(WpPrefixed {
            bcrypt: bcrypt.clone(),
            argon2: argon2.clone(),
            limits,
        }),
        Box::new(PhpassRejected),
        Box::new(BcryptFormat(bcrypt, limits)),
        Box::new(Argon2Format(argon2, limits)),
        Box::new(Unrecognized),
    ]
}

/// WordPress's `wp_check_password()`, without MD5 or phpass.
pub struct DefaultPasswordChecker {
    ports: Rc<PasswordPorts>,
}

impl DefaultPasswordChecker {
    pub fn new(ports: Rc<PasswordPorts>) -> DefaultPasswordChecker {
        DefaultPasswordChecker { ports }
    }
}

impl PasswordChecker for DefaultPasswordChecker {
    fn check(&self, password: &[u8], hash: &[u8], user_id: &Value) -> Result<Value, PasswordError> {
        let ports = &*self.ports;
        let chain = default_chain(
            ports.bcrypt.clone(),
            ports.argon2.clone(),
            ports.hasher_global.is_set(),
            ports.limits.limits()?,
        );
        let handled = chain
            .iter()
            .find_map(|link| link.handle(password, hash))
            .unwrap_or(Handled::Answer(false));
        let check = match handled {
            Handled::Answer(answer) => Value::Bool(answer),
            Handled::Delegate => ports
                .hasher_global
                .check(password, hash)
                .unwrap_or(Ok(Value::Bool(false)))?,
        };
        ports.filters.check_password(check, password, hash, user_id)
    }
}
