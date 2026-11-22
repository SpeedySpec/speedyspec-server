//! `wp_password_needs_rehash()`, over PHP's `password_needs_rehash()` rules.

use std::rc::Rc;

use speedyspec_hooks::Value;
use speedyspec_wp_password_contracts::{PasswordError, RehashPolicy};

use crate::algorithm::{
    Algorithm, algorithm_type_error, is_wp_bcrypt, resolve_algorithm, resolve_options,
};
use crate::hash::{
    ARGON2_DEFAULT_MEMORY_KIB, ARGON2_DEFAULT_THREADS, ARGON2_DEFAULT_TIME_COST,
    BCRYPT_DEFAULT_COST, DEFAULT_ALGORITHM, PasswordPorts,
};

const FUNCTION: &str = "password_needs_rehash";

/// PHP's `password_needs_rehash( $hash, $algorithm, $options )`.
///
/// A hash of another algorithm needs a rehash; one of the same algorithm needs one when its parameters differ from
/// the options (bcrypt's cost; argon2's memory, time and threads). An algorithm PHP doesn't know never asks for a
/// rehash.
///
/// # Errors
///
/// PHP's `TypeError`s for an array or object algorithm and for options that aren't an array.
pub fn password_needs_rehash(
    hash: &[u8],
    algorithm: &Value,
    options: &Value,
) -> Result<bool, PasswordError> {
    if let Some(err) = algorithm_type_error(algorithm, FUNCTION) {
        return Err(err);
    }
    let options = resolve_options(options, FUNCTION)?;
    let Ok(wanted) = resolve_algorithm(algorithm, FUNCTION) else {
        return Ok(false);
    };
    if identify(hash) != Some(wanted) {
        return Ok(true);
    }
    Ok(match wanted {
        Algorithm::Bcrypt => match leading_number(&hash[4..]) {
            Some((cost, _)) => cost != options.cost.unwrap_or(BCRYPT_DEFAULT_COST),
            None => true,
        },
        Algorithm::Argon2i | Algorithm::Argon2id => {
            let (memory, time, threads) = argon2_params(hash);
            memory != options.memory_cost.unwrap_or(ARGON2_DEFAULT_MEMORY_KIB)
                || time != options.time_cost.unwrap_or(ARGON2_DEFAULT_TIME_COST)
                || threads != options.threads.unwrap_or(ARGON2_DEFAULT_THREADS)
        }
    })
}

/// The algorithm PHP identifies a hash as (`password_get_info()`): its identifier between the first two `$`, when
/// the hash is valid for it (bcrypt: 60 bytes starting `$2y$`).
fn identify(hash: &[u8]) -> Option<Algorithm> {
    if hash.starts_with(b"$2y$") && hash.len() == 60 {
        Some(Algorithm::Bcrypt)
    } else if hash.starts_with(b"$argon2id$") {
        Some(Algorithm::Argon2id)
    } else if hash.starts_with(b"$argon2i$") {
        Some(Algorithm::Argon2i)
    } else {
        None
    }
}

/// `sscanf`'s `%ld` at the start of `text`: the number and what follows it.
fn leading_number(text: &[u8]) -> Option<(i64, &[u8])> {
    let digits = text
        .iter()
        .enumerate()
        .take_while(|(i, b)| b.is_ascii_digit() || (*i == 0 && matches!(b, b'-' | b'+')))
        .count();
    let number = std::str::from_utf8(&text[..digits]).ok()?.parse().ok()?;
    Some((number, &text[digits..]))
}

/// An argon2 hash's `m`, `t` and `p`, read as PHP's `sscanf("$%*[argon2id]$v=%ld$m=%ld,t=%ld,p=%ld")` does; a
/// parameter it can't reach reads as 0.
fn argon2_params(hash: &[u8]) -> (i64, i64, i64) {
    let mut values = [0i64; 3];
    let Some(rest) = hash[1..]
        .iter()
        .position(|b| *b == b'$')
        .map(|i| &hash[i + 2..])
    else {
        return (0, 0, 0);
    };
    let mut rest = match rest.strip_prefix(b"v=").and_then(leading_number) {
        Some((_, after)) => after,
        None => return (0, 0, 0),
    };
    for (i, prefix) in [&b"$m="[..], b",t=", b",p="].iter().enumerate() {
        match rest.strip_prefix(*prefix).and_then(leading_number) {
            Some((n, after)) => {
                values[i] = n;
                rest = after;
            }
            None => break,
        }
    }
    (values[0], values[1], values[2])
}

/// WordPress's `wp_password_needs_rehash()`, with argon2id as the default algorithm.
pub struct DefaultRehashPolicy {
    ports: Rc<PasswordPorts>,
}

impl DefaultRehashPolicy {
    pub fn new(ports: Rc<PasswordPorts>) -> DefaultRehashPolicy {
        DefaultRehashPolicy { ports }
    }
}

impl RehashPolicy for DefaultRehashPolicy {
    fn needs_rehash(&self, hash: &[u8], user_id: &Value) -> Result<Value, PasswordError> {
        let ports = &*self.ports;
        if ports.hasher_global.is_set() {
            return Ok(Value::Bool(false));
        }
        let algorithm = ports
            .filters
            .algorithm(Value::Str(DEFAULT_ALGORITHM.into()))?;
        let options = ports
            .filters
            .options(Value::Array(Vec::new()), &algorithm)?;
        let prefixed = hash.starts_with(b"$wp");
        let needs = if is_wp_bcrypt(&algorithm) && !prefixed {
            true
        } else {
            let unprefixed = if prefixed { &hash[3..] } else { hash };
            password_needs_rehash(unprefixed, &algorithm, &options)?
        };
        ports.filters.needs_rehash(needs, hash, user_id)
    }
}
