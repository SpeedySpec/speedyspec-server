//! Application passwords' hash, check and chunking.

mod common;

use std::cell::RefCell;

use common::*;
use speedyspec_hooks::Value;
use speedyspec_wp_crypto_contracts::{Base64UrlNoPad, GenericHash};
use speedyspec_wp_password::{
    application_password_check, application_password_hash, chunk_password,
};
use speedyspec_wp_password_contracts::{PasswordChecker, PasswordError};

struct XorHash;

impl GenericHash for XorHash {
    fn hash(&self, message: &[u8], key: &[u8], out_len: usize) -> Vec<u8> {
        let mut out = vec![0u8; out_len];
        for (i, b) in message.iter().chain(key).enumerate() {
            out[i % out_len] ^= b;
        }
        out
    }
}

struct Hex;

impl Base64UrlNoPad for Hex {
    fn encode(&self, bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }
}

struct Checker(RefCell<Vec<Vec<u8>>>, Value);

impl PasswordChecker for Checker {
    fn check(
        &self,
        _password: &[u8],
        hash: &[u8],
        user_id: &Value,
    ) -> Result<Value, PasswordError> {
        assert_eq!(*user_id, Value::Str(String::new()));
        self.0.borrow_mut().push(hash.to_vec());
        Ok(self.1.clone())
    }
}

#[test]
fn application_hash_is_fast_hash() {
    let hash = application_password_hash(&XorHash, &Hex, b"secret");
    assert_eq!(
        hash,
        speedyspec_wp_crypto::wp_fast_hash(&XorHash, &Hex, b"secret")
    );
    assert!(hash.starts_with("$generic$"));
}

#[test]
fn application_check_generic_uses_fast_hash() {
    let checker = Checker(RefCell::new(vec![]), Value::Bool(true));
    let stored = application_password_hash(&XorHash, &Hex, b"secret");
    assert!(
        application_password_check(&checker, &XorHash, &Hex, b"secret", stored.as_bytes()).unwrap()
    );
    assert!(
        !application_password_check(&checker, &XorHash, &Hex, b"wrong", stored.as_bytes()).unwrap()
    );
    assert!(checker.0.borrow().is_empty());
}

#[test]
fn application_check_other_uses_bound_checker() {
    // WP_Application_Passwords::check_password() returns bool: the checker's value is coerced.
    let checker = Checker(RefCell::new(vec![]), Value::Int(1));
    assert!(application_password_check(&checker, &XorHash, &Hex, b"pw", b"$2y$10$stored").unwrap());
    assert_eq!(*checker.0.borrow(), vec![b"$2y$10$stored".to_vec()]);
}

#[test]
fn chunk_replays_recorded() {
    for case in cases()["chunk"].as_array().unwrap() {
        assert_eq!(
            chunk_password(case["raw"].as_str().unwrap().as_bytes()),
            case["result"].as_str().unwrap().as_bytes(),
            "{}",
            case["name"]
        );
    }
}
