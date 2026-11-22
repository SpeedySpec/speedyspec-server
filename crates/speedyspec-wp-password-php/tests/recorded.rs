//! WordPress's password rules over the real crypto (libsodium's argon2, the bcrypt adapter), replaying what
//! `compat/password/record.php` recorded: every hash the rules write verifies, and every argon2 hash PHP wrote
//! verifies as PHP said.

use std::path::PathBuf;
use std::rc::Rc;

use serde_json::Value as Json;
use speedyspec_hooks::{Key, Value};
use speedyspec_wp_crypto_sodium::Sodium;
use speedyspec_wp_password::{
    DefaultPasswordChecker, DefaultPasswordHasher, PasswordPorts, php_trim,
};
use speedyspec_wp_password_bcrypt::BcryptCrate;
use speedyspec_wp_password_contracts::{
    PasswordChecker, PasswordError, PasswordFilters, PasswordHasher, Warnings, WpHasherGlobal,
};

fn cases() -> Json {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../compat/password/cases.json");
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn unhex(text: &str) -> Vec<u8> {
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).unwrap())
        .collect()
}

fn password(json: &Json) -> Vec<u8> {
    match json {
        Json::String(hex) => unhex(hex),
        Json::Object(o) => {
            unhex(o["repeat"].as_str().unwrap()).repeat(o["times"].as_u64().unwrap() as usize)
        }
        other => panic!("{other}"),
    }
}

fn value(json: &Json) -> Value {
    match json {
        Json::Null => Value::Null,
        Json::Bool(b) => Value::Bool(*b),
        Json::Number(n) if !n.to_string().contains('.') => Value::Int(n.as_i64().unwrap()),
        Json::Number(n) => Value::Float(n.as_f64().unwrap()),
        Json::String(s) => Value::Str(s.clone()),
        Json::Array(a) => Value::Array(
            a.iter()
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

/// The algorithm and options filters answering fixed values; the others pass their value through.
struct Fixed(Option<Value>, Option<Value>);

impl PasswordFilters for Fixed {
    fn algorithm(&self, default: Value) -> Result<Value, PasswordError> {
        Ok(self.0.clone().unwrap_or(default))
    }
    fn options(&self, options: Value, _: &Value) -> Result<Value, PasswordError> {
        Ok(self.1.clone().unwrap_or(options))
    }
    fn check_password(
        &self,
        check: Value,
        _: &[u8],
        _: &[u8],
        _: &Value,
    ) -> Result<Value, PasswordError> {
        Ok(check)
    }
    fn needs_rehash(&self, needs: bool, _: &[u8], _: &Value) -> Result<Value, PasswordError> {
        Ok(Value::Bool(needs))
    }
    fn random_password(
        &self,
        p: Vec<u8>,
        _: &Value,
        _: &Value,
        _: &Value,
    ) -> Result<Value, PasswordError> {
        Ok(Value::Bytes(p))
    }
}

struct NoHasher;

impl WpHasherGlobal for NoHasher {
    fn is_set(&self) -> bool {
        false
    }
    fn hash(&self, _: &[u8]) -> Option<Result<Value, PasswordError>> {
        None
    }
    fn check(&self, _: &[u8], _: &[u8]) -> Option<Result<Value, PasswordError>> {
        None
    }
}

struct Quiet;

impl Warnings for Quiet {
    fn warn(&self, _: &str) -> Result<(), PasswordError> {
        Ok(())
    }
}

fn ports(algorithm: Option<Value>, options: Option<Value>) -> Rc<PasswordPorts> {
    let sodium = Rc::new(Sodium::init().unwrap());
    Rc::new(PasswordPorts {
        filters: Rc::new(Fixed(algorithm, options)),
        hasher_global: Rc::new(NoHasher),
        bcrypt: Rc::new(BcryptCrate),
        argon2: sodium.clone(),
        random: sodium,
        warnings: Rc::new(Quiet),
        limits: Rc::new(speedyspec_wp_password::FixedCostLimits::default()),
    })
}

#[test]
fn written_hashes_verify() {
    let cases = cases();
    let mut checked = 0;
    for case in cases["hash"]
        .as_array()
        .unwrap()
        .iter()
        .chain(cases["options"].as_array().unwrap())
    {
        let name = case["name"].as_str().unwrap();
        let ports = ports(
            case["algorithm"].get("value").map(value),
            case.get("options").map(value),
        );
        let p = case
            .get("password")
            .map(password)
            .unwrap_or_else(|| b"password".to_vec());
        let Ok(Value::Str(hash)) = DefaultPasswordHasher::new(ports.clone()).hash(&p) else {
            continue;
        };
        let checker = DefaultPasswordChecker::new(ports);
        let plain_bcrypt = hash.starts_with("$2y$");
        let attempt = if plain_bcrypt { &p[..] } else { php_trim(&p) };
        let verified = checker
            .check(attempt, hash.as_bytes(), &Value::Null)
            .unwrap();
        assert_eq!(verified, Value::Bool(hash != "*"), "{name}: {hash}");
        checked += 1;
    }
    assert!(checked > 40, "{checked}");
}

#[test]
fn php_argon2_hashes_verify_as_php_said() {
    let checker = DefaultPasswordChecker::new(ports(None, None));
    for case in cases()["argon2_php"].as_array().unwrap() {
        let p = password(&case["password"]);
        let hash = case["hash"].as_str().unwrap().as_bytes();
        let mut wrong = p.clone();
        wrong.push(b'x');
        assert_eq!(
            checker.check(&p, hash, &Value::Null).unwrap(),
            Value::Bool(case["verify_right"].as_bool().unwrap()),
            "{}",
            case["name"]
        );
        assert_eq!(
            checker.check(&wrong, hash, &Value::Null).unwrap(),
            Value::Bool(false),
            "{}",
            case["name"]
        );
    }
}
