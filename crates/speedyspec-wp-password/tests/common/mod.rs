//! Test doubles for the password ports, and the recorded corpus.
#![allow(dead_code)]

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use serde_json::Value as Json;
use speedyspec_hooks::{Key, Value};
use speedyspec_wp_crypto_contracts::{Argon2, Argon2Variant, RandomBytes};
use speedyspec_wp_password_contracts::{
    Bcrypt, PasswordError, PasswordFilters, Warnings, WpHasherGlobal,
};

pub fn cases() -> Json {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../compat/password/cases.json");
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

pub fn unhex(text: &str) -> Vec<u8> {
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).unwrap())
        .collect()
}

/// A recorded password: hex, or `{repeat, times}`.
pub fn password(json: &Json) -> Vec<u8> {
    match json {
        Json::String(hex) => unhex(hex),
        Json::Object(o) => {
            unhex(o["repeat"].as_str().unwrap()).repeat(o["times"].as_u64().unwrap() as usize)
        }
        other => panic!("not a password: {other}"),
    }
}

/// A recorded PHP value as a hook value.
pub fn value(json: &Json) -> Value {
    match json {
        Json::Null => Value::Null,
        Json::Bool(b) => Value::Bool(*b),
        Json::Number(n) => match n.as_i64() {
            Some(i) if !n.to_string().contains('.') => Value::Int(i),
            _ => Value::Float(n.as_f64().unwrap()),
        },
        Json::String(s) => Value::Str(s.clone()),
        Json::Array(items) => Value::Array(
            items
                .iter()
                .enumerate()
                .map(|(i, v)| (Key::Int(i as i64), value(v)))
                .collect(),
        ),
        Json::Object(o) => Value::Array(
            o.iter()
                .map(|(k, v)| (Key::Str(k.clone()), value(v)))
                .collect(),
        ),
    }
}

pub fn calls() -> Rc<RefCell<Vec<String>>> {
    Rc::new(RefCell::new(Vec::new()))
}

/// Filters returning configured values (or passing defaults through), recording each call.
pub struct FakeFilters {
    pub algorithm: Option<Value>,
    pub options: Option<Value>,
    pub calls: Rc<RefCell<Vec<String>>>,
    pub seen_algorithm_for_options: RefCell<Option<Value>>,
    pub check_args: RefCell<Option<(Value, Vec<u8>, Vec<u8>, Value)>>,
    pub rehash_args: RefCell<Option<(bool, Vec<u8>, Value)>>,
    pub random_args: RefCell<Option<(Vec<u8>, i64, bool, bool)>>,
}

impl FakeFilters {
    pub fn new(calls: Rc<RefCell<Vec<String>>>) -> FakeFilters {
        FakeFilters {
            algorithm: None,
            options: None,
            calls,
            seen_algorithm_for_options: RefCell::new(None),
            check_args: RefCell::new(None),
            rehash_args: RefCell::new(None),
            random_args: RefCell::new(None),
        }
    }
    pub fn with(
        algorithm: Option<Value>,
        options: Option<Value>,
        calls: Rc<RefCell<Vec<String>>>,
    ) -> FakeFilters {
        FakeFilters {
            algorithm,
            options,
            ..FakeFilters::new(calls)
        }
    }
}

impl PasswordFilters for FakeFilters {
    fn algorithm(&self, default: Value) -> Result<Value, PasswordError> {
        self.calls.borrow_mut().push("algorithm".into());
        Ok(self.algorithm.clone().unwrap_or(default))
    }
    fn options(&self, options: Value, algorithm: &Value) -> Result<Value, PasswordError> {
        self.calls.borrow_mut().push("options".into());
        *self.seen_algorithm_for_options.borrow_mut() = Some(algorithm.clone());
        Ok(self.options.clone().unwrap_or(options))
    }
    fn check_password(
        &self,
        check: Value,
        password: &[u8],
        hash: &[u8],
        user_id: &Value,
    ) -> Result<Value, PasswordError> {
        self.calls.borrow_mut().push("check_password".into());
        *self.check_args.borrow_mut() = Some((
            check.clone(),
            password.to_vec(),
            hash.to_vec(),
            user_id.clone(),
        ));
        Ok(check)
    }
    fn needs_rehash(
        &self,
        needs: bool,
        hash: &[u8],
        user_id: &Value,
    ) -> Result<Value, PasswordError> {
        self.calls.borrow_mut().push("password_needs_rehash".into());
        *self.rehash_args.borrow_mut() = Some((needs, hash.to_vec(), user_id.clone()));
        Ok(Value::Bool(needs))
    }
    fn random_password(
        &self,
        password: Vec<u8>,
        length: i64,
        special: bool,
        extra: bool,
    ) -> Result<Value, PasswordError> {
        self.calls.borrow_mut().push("random_password".into());
        *self.random_args.borrow_mut() = Some((password.clone(), length, special, extra));
        Ok(Value::Str(String::from_utf8(password).unwrap()))
    }
}

/// `$wp_hasher`: empty, or an object answering fixed values.
pub struct FakeHasher {
    pub set: bool,
    pub calls: Rc<RefCell<Vec<String>>>,
}

impl WpHasherGlobal for FakeHasher {
    fn is_set(&self) -> bool {
        self.set
    }
    fn hash(&self, trimmed: &[u8]) -> Option<Result<Value, PasswordError>> {
        self.set.then(|| {
            self.calls.borrow_mut().push("HashPassword".into());
            Ok(Value::Str(format!(
                "H:{}",
                String::from_utf8_lossy(trimmed)
            )))
        })
    }
    fn check(&self, _password: &[u8], _hash: &[u8]) -> Option<Result<Value, PasswordError>> {
        self.set.then(|| {
            self.calls.borrow_mut().push("CheckPassword".into());
            Ok(Value::Bool(true))
        })
    }
}

/// bcrypt that records its input and answers a well-formed `$2y$` hash.
pub struct FakeBcrypt {
    pub seen: RefCell<Vec<(Vec<u8>, u32)>>,
}

impl FakeBcrypt {
    pub fn new() -> FakeBcrypt {
        FakeBcrypt {
            seen: RefCell::new(Vec::new()),
        }
    }
}

impl Bcrypt for FakeBcrypt {
    fn hash(&self, password: &[u8], cost: u32, _salt: &[u8; 16]) -> Result<String, String> {
        self.seen.borrow_mut().push((password.to_vec(), cost));
        Ok(format!("$2y${cost:02}${}", ".".repeat(53)))
    }
    fn verify(&self, password: &[u8], hash: &[u8]) -> bool {
        self.seen.borrow_mut().push((password.to_vec(), 0));
        hash.starts_with(b"$2y$") && password == b"right"
    }
}

/// argon2 that records its input and answers bytes derived from it.
pub struct FakeArgon2 {
    pub seen: RefCell<Vec<(Argon2Variant, Vec<u8>, u32, u32)>>,
}

impl FakeArgon2 {
    pub fn new() -> FakeArgon2 {
        FakeArgon2 {
            seen: RefCell::new(Vec::new()),
        }
    }
}

impl Argon2 for FakeArgon2 {
    fn derive(
        &self,
        variant: Argon2Variant,
        password: &[u8],
        salt: &[u8; 16],
        time_cost: u32,
        memory_kib: u32,
        out_len: usize,
    ) -> Result<Vec<u8>, String> {
        self.seen
            .borrow_mut()
            .push((variant, password.to_vec(), time_cost, memory_kib));
        let mut out = vec![0u8; out_len];
        for (i, b) in password.iter().chain(salt).enumerate() {
            out[i % out_len] ^= b.wrapping_add(time_cost as u8);
        }
        Ok(out)
    }
}

pub struct FixedBytes(pub u8);

impl RandomBytes for FixedBytes {
    fn fill(&self, out: &mut [u8]) {
        out.fill(self.0);
    }
}

pub struct CollectWarnings(pub RefCell<Vec<String>>);

impl Warnings for CollectWarnings {
    fn warn(&self, message: &str) {
        self.0.borrow_mut().push(message.to_string());
    }
}
