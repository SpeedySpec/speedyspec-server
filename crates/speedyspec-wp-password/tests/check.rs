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
use speedyspec_wp_password::{
    DefaultPasswordChecker, FixedCostLimits, PasswordPorts, argon2_phc, prehash,
};
use speedyspec_wp_password_bcrypt::BcryptCrate;
use speedyspec_wp_password_contracts::{
    Bcrypt, CostLimits, PasswordChecker, PasswordCostLimits, PasswordError,
};

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
        limits: Rc::new(speedyspec_wp_password::FixedCostLimits::default()),
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

/// A checker with `limits`, over a bcrypt double (which verifies the password "right") and the argon2 double.
fn limited_checker(
    limits: Rc<dyn PasswordCostLimits>,
) -> (DefaultPasswordChecker, Rc<FakeBcrypt>, Rc<FakeArgon2>) {
    let bcrypt = Rc::new(FakeBcrypt::new());
    let argon2 = Rc::new(FakeArgon2::new());
    let checker = DefaultPasswordChecker::new(Rc::new(PasswordPorts {
        filters: Rc::new(FakeFilters::new(calls())),
        hasher_global: Rc::new(FakeHasher {
            set: false,
            calls: calls(),
        }),
        bcrypt: bcrypt.clone(),
        argon2: argon2.clone(),
        random: Rc::new(FixedBytes(1)),
        warnings: Rc::new(CollectWarnings(Default::default())),
        limits,
    }));
    (checker, bcrypt, argon2)
}

fn verdict(checker: &DefaultPasswordChecker, password: &[u8], hash: &str) -> Value {
    checker
        .check(password, hash.as_bytes(), &Value::Str(String::new()))
        .unwrap()
}

#[test]
fn bcrypt_above_the_cost_limit_never_verifies_nor_runs() {
    let (checker, bcrypt, _) = limited_checker(Rc::new(FixedCostLimits::default()));
    let tail = ".".repeat(53);
    for hash in [
        format!("$2y$17${tail}"),
        format!("$2a$31${tail}"),
        format!("$2b$20${tail}"),
        format!("$wp$2y$17${tail}"),
    ] {
        assert_eq!(
            verdict(&checker, b"right", &hash),
            Value::Bool(false),
            "{hash}"
        );
    }
    assert!(bcrypt.seen.borrow().is_empty());
    assert_eq!(
        verdict(&checker, b"right", &format!("$2y$16${tail}")),
        Value::Bool(true)
    );
}

fn argon2_hash(time_cost: u32, memory_kib: u32) -> String {
    let salt = [5u8; 16];
    let digest = FakeArgon2::new()
        .derive(
            Argon2Variant::Argon2id,
            b"pw",
            &salt,
            time_cost,
            memory_kib,
            32,
        )
        .unwrap();
    argon2_phc(
        Argon2Variant::Argon2id,
        time_cost,
        memory_kib,
        &salt,
        &digest,
    )
}

#[test]
fn argon2_above_the_cost_limits_never_verifies_nor_runs() {
    let (checker, _, argon2) = limited_checker(Rc::new(FixedCostLimits::default()));
    for hash in [argon2_hash(10, 262_145), argon2_hash(11, 262_144)] {
        assert_eq!(
            verdict(&checker, b"pw", &hash),
            Value::Bool(false),
            "{hash}"
        );
        let prefixed = format!("$wp{hash}");
        assert_eq!(
            verdict(&checker, b"pw", &prefixed),
            Value::Bool(false),
            "{prefixed}"
        );
    }
    assert!(argon2.seen.borrow().is_empty());
    assert_eq!(
        verdict(&checker, b"pw", &argon2_hash(10, 262_144)),
        Value::Bool(true)
    );
}

#[test]
fn a_sites_cost_limits_replace_the_defaults() {
    let (checker, _, _) = limited_checker(Rc::new(FixedCostLimits(CostLimits {
        bcrypt_cost: 10,
        argon2_memory_kib: 4096,
        argon2_time_cost: 3,
    })));
    let tail = ".".repeat(53);
    assert_eq!(
        verdict(&checker, b"right", &format!("$2y$11${tail}")),
        Value::Bool(false)
    );
    assert_eq!(
        verdict(&checker, b"right", &format!("$2y$10${tail}")),
        Value::Bool(true)
    );
    assert_eq!(
        verdict(&checker, b"pw", &argon2_hash(3, 4097)),
        Value::Bool(false)
    );
    assert_eq!(
        verdict(&checker, b"pw", &argon2_hash(4, 4096)),
        Value::Bool(false)
    );
    assert_eq!(
        verdict(&checker, b"pw", &argon2_hash(3, 4096)),
        Value::Bool(true)
    );
}

/// Limits that fail, as a broken replacement does.
struct BrokenLimits;

impl PasswordCostLimits for BrokenLimits {
    fn limits(&self) -> Result<CostLimits, PasswordError> {
        Err(PasswordError::Port("no limits".into()))
    }
}

#[test]
fn failing_limits_stop_the_check() {
    let (checker, _, _) = limited_checker(Rc::new(BrokenLimits));
    assert_eq!(
        checker.check(
            b"right",
            format!("$2y$10${}", ".".repeat(53)).as_bytes(),
            &Value::Null
        ),
        Err(PasswordError::Port("no limits".into()))
    );
}
