//! `wp_check_password()`'s chain of hash-format handlers, replayed against what stock WordPress answered.
//!
//! bcrypt is the real (pure Rust) adapter; argon2 is a test double here, and the argon2 formats are replayed over
//! libsodium in `speedyspec-wp-password-php`'s recorded tests.

mod common;

use std::cell::RefCell;
use std::rc::Rc;

use common::*;
use speedyspec_hooks::Value;
use speedyspec_wp_crypto_contracts::{Argon2, Argon2Variant};
use speedyspec_wp_password::{DefaultPasswordChecker, PasswordPorts, argon2_phc, prehash};
use speedyspec_wp_password_bcrypt::BcryptCrate;
use speedyspec_wp_password_contracts::{Bcrypt, PasswordChecker};

/// Formats WordPress verifies and SpeedySpec rejects on purpose: MD5 and phpass (Jacob, 2026-11-21/22), `$2x$`, and
/// the `crypt()`-only formats.
const DIVERGENCES: &[&str] = &[
    "md5",
    "phpass",
    "2x",
    "md5-crypt",
    "sha256-crypt",
    "sha512-crypt",
];

struct Rig {
    checker: DefaultPasswordChecker,
    filters: Rc<FakeFilters>,
    argon2: Rc<FakeArgon2>,
    calls: Rc<RefCell<Vec<String>>>,
}

fn rig(hasher_set: bool) -> Rig {
    let calls = calls();
    let filters = Rc::new(FakeFilters::new(calls.clone()));
    let argon2 = Rc::new(FakeArgon2::new());
    let ports = Rc::new(PasswordPorts {
        filters: filters.clone(),
        hasher_global: Rc::new(FakeHasher {
            set: hasher_set,
            calls: calls.clone(),
        }),
        bcrypt: Rc::new(BcryptCrate),
        argon2: argon2.clone(),
        random: Rc::new(FixedBytes(1)),
        warnings: Rc::new(CollectWarnings(Default::default())),
    });
    Rig {
        checker: DefaultPasswordChecker::new(ports),
        filters,
        argon2,
        calls,
    }
}

fn check(rig: &Rig, password: &[u8], hash: &[u8]) -> Value {
    rig.checker
        .check(password, hash, &Value::Str(String::new()))
        .unwrap()
}

#[test]
fn check_replays_recorded() {
    let cases = cases();
    let mut replayed = 0;
    for case in cases["check"].as_array().unwrap() {
        let format = case["format"].as_str().unwrap();
        if format.contains("argon2") {
            continue;
        }
        let name = case["name"].as_str().unwrap();
        let got = check(
            &rig(false),
            &password(&case["password"]),
            case["hash"].as_str().unwrap().as_bytes(),
        );
        let wordpress = case["result"].as_bool().unwrap();
        let expected = if DIVERGENCES.contains(&format) {
            false
        } else {
            wordpress
        };
        assert_eq!(got, Value::Bool(expected), "{name}");
        replayed += 1;
    }
    assert!(replayed > 80);
}

#[test]
fn md5_rejected_even_when_hash_matches() {
    let rig = rig(false);
    // md5('password')
    assert_eq!(
        check(&rig, b"password", b"5f4dcc3b5aa765d61d8327deb882cf99"),
        Value::Bool(false)
    );
    assert_eq!(
        rig.filters.check_args.borrow().as_ref().unwrap().0,
        Value::Bool(false)
    );
}

#[test]
fn length_rule_precedes_wp_hasher() {
    let rig = rig(true);
    assert_eq!(check(&rig, b"password", &[b'x'; 32]), Value::Bool(false));
    assert!(!rig.calls.borrow().contains(&"CheckPassword".to_string()));
}

#[test]
fn wp_hasher_delegate() {
    let rig = rig(true);
    assert_eq!(check(&rig, b"anything", &[b'x'; 60]), Value::Bool(true));
    assert_eq!(
        *rig.calls.borrow(),
        vec!["CheckPassword".to_string(), "check_password".to_string()]
    );
}

#[test]
fn filter_receives_user_id_untouched() {
    let rig = rig(false);
    rig.checker
        .check(b"pw", &[b'x'; 40], &Value::Int(7))
        .unwrap();
    let args = rig.filters.check_args.borrow().clone().unwrap();
    assert_eq!((args.1, args.3), (b"pw".to_vec(), Value::Int(7)));
}

#[test]
fn wp_prefix_untrimmed() {
    let rig = rig(false);
    let hash = format!(
        "$wp{}",
        BcryptCrate
            .hash(prehash(b"pw").as_bytes(), 4, &[3; 16])
            .unwrap()
    );
    assert_eq!(check(&rig, b"pw", hash.as_bytes()), Value::Bool(true));
    assert_eq!(check(&rig, b" pw", hash.as_bytes()), Value::Bool(false));
}

#[test]
fn too_long_password_never_verifies() {
    let rig = rig(false);
    let long = vec![b'a'; 4097];
    let hash = BcryptCrate.hash(&long, 4, &[3; 16]).unwrap();
    assert_eq!(check(&rig, &long, hash.as_bytes()), Value::Bool(false));
}

#[test]
fn argon2_verifies_through_port() {
    let rig = rig(false);
    let salt = [5u8; 16];
    let digest = FakeArgon2::new()
        .derive(Argon2Variant::Argon2id, b"pw", &salt, 3, 4096, 32)
        .unwrap();
    let hash = argon2_phc(Argon2Variant::Argon2id, 3, 4096, &salt, &digest);
    assert_eq!(check(&rig, b"pw", hash.as_bytes()), Value::Bool(true));
    assert_eq!(check(&rig, b"px", hash.as_bytes()), Value::Bool(false));
    let prefixed = format!(
        "$wp{}",
        argon2_phc(
            Argon2Variant::Argon2id,
            3,
            4096,
            &salt,
            &FakeArgon2::new()
                .derive(
                    Argon2Variant::Argon2id,
                    prehash(b"pw").as_bytes(),
                    &salt,
                    3,
                    4096,
                    32
                )
                .unwrap()
        )
    );
    assert_eq!(check(&rig, b"pw", prefixed.as_bytes()), Value::Bool(true));
}

#[test]
fn argon2_unsupported_params_do_not_verify() {
    let rig = rig(false);
    let salt = "AQEBAQEBAQEBAQEBAQEBAQ";
    let digest = "AQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQE";
    for hash in [
        format!("$argon2id$v=19$m=4096,t=3,p=2${salt}${digest}"),
        format!("$argon2id$m=4096,t=3,p=1${salt}${digest}"),
        format!("$argon2i$v=19$m=4096,t=2,p=1${salt}${digest}"),
        format!("$argon2id$v=19$m=4096,t=3,p=1$AQEB${digest}"),
        format!("$argon2id$v=19$m=4096,t=3${salt}${digest}"),
        "$argon2id$v=19$m=4096,t=3,p=1$!!$!!".to_string(),
    ] {
        assert_eq!(
            check(&rig, b"pw", hash.as_bytes()),
            Value::Bool(false),
            "{hash}"
        );
    }
    assert!(rig.argon2.seen.borrow().is_empty());
}
