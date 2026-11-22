//! `wp_hash_password()`'s rules, with test doubles for every port, replayed against what stock WordPress did.

mod common;

use std::rc::Rc;

use common::*;
use speedyspec_hooks::{Key, Value};
use speedyspec_wp_crypto_contracts::Argon2Variant;
use speedyspec_wp_password::{
    DEFAULT_ALGORITHM, DefaultPasswordHasher, PasswordPorts, argon2_phc, parse_phc, prehash,
};
use speedyspec_wp_password_contracts::{PasswordError, PasswordHasher};

/// Cases where SpeedySpec answers differently on purpose (see the design's divergence table).
///
/// - `hash/default/`: argon2id is the default, not bcrypt (Jacob, 2026-11-22).
/// - `options/argon2id/threads-2`, `options/argon2i/t-1`, `options/argon2i/t-2`: libsodium computes one lane only
///   and argon2i from three passes; the runtime's PHP hashes these with libargon2.
const DIVERGENCES: &[&str] = &[
    "hash/default/",
    "options/argon2id/threads-2",
    "options/argon2i/t-1",
    "options/argon2i/t-2",
];

struct Rig {
    hasher: DefaultPasswordHasher,
    filters: Rc<FakeFilters>,
    bcrypt: Rc<FakeBcrypt>,
    argon2: Rc<FakeArgon2>,
    warnings: Rc<CollectWarnings>,
    calls: std::rc::Rc<std::cell::RefCell<Vec<String>>>,
}

fn rig(algorithm: Option<Value>, options: Option<Value>, hasher_set: bool) -> Rig {
    let calls = calls();
    let filters = Rc::new(FakeFilters::with(algorithm, options, calls.clone()));
    let bcrypt = Rc::new(FakeBcrypt::new());
    let argon2 = Rc::new(FakeArgon2::new());
    let warnings = Rc::new(CollectWarnings(Default::default()));
    let ports = Rc::new(PasswordPorts {
        filters: filters.clone(),
        hasher_global: Rc::new(FakeHasher {
            set: hasher_set,
            calls: calls.clone(),
        }),
        bcrypt: bcrypt.clone(),
        argon2: argon2.clone(),
        random: Rc::new(FixedBytes(1)),
        warnings: warnings.clone(),
    });
    Rig {
        hasher: DefaultPasswordHasher::new(ports),
        filters,
        bcrypt,
        argon2,
        warnings,
        calls,
    }
}

fn text(value: Value) -> String {
    match value {
        Value::Str(s) => s,
        other => panic!("not a string: {other:?}"),
    }
}

/// A hash's shape without its salt: bcrypt up to its cost, argon2 up to its parameters.
fn shape(hash: &str) -> String {
    let without_tail = &hash[..hash.rfind('$').map_or(0, |i| i + 1)];
    if hash.contains("$argon2") {
        let cut: Vec<&str> = without_tail.split('$').collect();
        let keep = if hash.starts_with("$wp") { 5 } else { 4 };
        cut[..keep.min(cut.len())].join("$") + "$"
    } else {
        without_tail.to_string()
    }
}

#[test]
fn prehash_matches_recorded() {
    for case in cases()["prehash"].as_array().unwrap() {
        assert_eq!(
            prehash(&password(&case["password"])),
            case["result"].as_str().unwrap(),
            "{}",
            case["name"]
        );
    }
}

#[test]
fn default_is_argon2id_trimmed() {
    let rig = rig(None, None, false);
    let hash = text(rig.hasher.hash(b" pw ").unwrap());
    assert_eq!(DEFAULT_ALGORITHM, "argon2id");
    assert!(
        hash.starts_with("$argon2id$v=19$m=65536,t=4,p=1$"),
        "{hash}"
    );
    assert_eq!(
        rig.argon2.seen.borrow()[0],
        (Argon2Variant::Argon2id, b"pw".to_vec(), 4, 65536)
    );
}

#[test]
fn argon2_phc_round_trips() {
    let salt = [7u8; 16];
    let digest = [9u8; 32];
    let phc = argon2_phc(Argon2Variant::Argon2i, 3, 4096, &salt, &digest);
    assert!(phc.starts_with("$argon2i$v=19$m=4096,t=3,p=1$"), "{phc}");
    let parsed = parse_phc(phc.as_bytes()).unwrap();
    assert_eq!(
        (
            parsed.variant,
            parsed.version,
            parsed.memory_kib,
            parsed.time_cost,
            parsed.threads
        ),
        (Argon2Variant::Argon2i, 19, 4096, 3, 1)
    );
    assert_eq!(
        (parsed.salt, parsed.digest),
        (salt.to_vec(), digest.to_vec())
    );
    // PHP's own hashes parse: salt 16 bytes, digest 32, base64 without padding.
    for case in cases()["argon2_php"].as_array().unwrap() {
        let hash = case["hash"].as_str().unwrap();
        let parsed = parse_phc(hash.as_bytes()).unwrap_or_else(|| panic!("{hash}"));
        assert_eq!((parsed.salt.len(), parsed.digest.len()), (16, 32));
        assert_eq!(
            argon2_phc(
                parsed.variant,
                parsed.time_cost,
                parsed.memory_kib,
                &parsed.salt.try_into().unwrap(),
                &parsed.digest
            ),
            hash
        );
    }
}

#[test]
fn strict_2y_is_wp_bcrypt() {
    let rig = rig(Some(Value::Str("2y".into())), None, false);
    let hash = text(rig.hasher.hash(b" pw ").unwrap());
    assert!(hash.starts_with("$wp$2y$12$"), "{hash}");
    assert_eq!(
        rig.bcrypt.seen.borrow()[0],
        (prehash(b"pw").into_bytes(), 12)
    );
}

#[test]
fn non_strict_bcrypt_is_plain_untrimmed() {
    for algorithm in [Value::Int(1), Value::Null] {
        let rig = rig(Some(algorithm), None, false);
        let hash = text(rig.hasher.hash(b" pw ").unwrap());
        assert!(hash.starts_with("$2y$12$"), "{hash}");
        assert_eq!(rig.bcrypt.seen.borrow()[0], (b" pw ".to_vec(), 12));
    }
}

#[test]
fn algorithm_and_options_replayed() {
    let cases = cases();
    let mut replayed = 0;
    for section in ["hash", "options"] {
        for case in cases[section].as_array().unwrap() {
            let name = case["name"].as_str().unwrap();
            let algorithm = case["algorithm"].get("value").map(value);
            let options = case.get("options").map(value);
            let rig = rig(algorithm, options, false);
            let result = rig.hasher.hash(&password(
                case.get("password")
                    .unwrap_or(&serde_json::json!("70617373776f7264")),
            ));
            if DIVERGENCES.iter().any(|d| name.starts_with(d)) {
                continue;
            }
            replayed += 1;
            match (&result, case.get("exception")) {
                (Err(PasswordError::Throw { class, message }), Some(e)) => {
                    assert_eq!(
                        (class.as_str(), message.as_str()),
                        (e["class"].as_str().unwrap(), e["message"].as_str().unwrap()),
                        "{name}"
                    );
                }
                (Ok(hash), None) => {
                    let hash = text(hash.clone());
                    assert_eq!(
                        shape(&hash),
                        shape(case["head"].as_str().unwrap()),
                        "{name}"
                    );
                }
                other => panic!("{name}: {other:?}"),
            }
            let warnings: Vec<String> = case["warnings"]
                .as_array()
                .unwrap()
                .iter()
                .map(|w| w.as_str().unwrap().to_string())
                .collect();
            assert_eq!(*rig.warnings.0.borrow(), warnings, "{name}");
        }
    }
    assert!(replayed > 80);
}

#[test]
fn divergent_cases_answer_as_designed() {
    let threads = rig(
        Some(Value::Str("argon2id".into())),
        Some(Value::Array(vec![(
            Key::Str("threads".into()),
            Value::Int(2),
        )])),
        false,
    );
    assert!(
        matches!(threads.hasher.hash(b"pw"), Err(PasswordError::Throw { class, .. }) if class == "ValueError")
    );
    let argon2i = rig(
        Some(Value::Str("argon2i".into())),
        Some(Value::Array(vec![(
            Key::Str("time_cost".into()),
            Value::Int(2),
        )])),
        false,
    );
    assert!(
        matches!(argon2i.hasher.hash(b"pw"), Err(PasswordError::Throw { class, .. }) if class == "ValueError")
    );
}

#[test]
fn too_long_is_star_without_filters() {
    let rig = rig(None, None, false);
    assert_eq!(
        rig.hasher.hash(&[b'a'; 4097]).unwrap(),
        Value::Str("*".into())
    );
    assert!(rig.calls.borrow().is_empty());
}

#[test]
fn wp_hasher_short_circuits() {
    let rig = rig(None, None, true);
    assert_eq!(
        rig.hasher.hash(b" pw\n").unwrap(),
        Value::Str("H:pw".into())
    );
    assert_eq!(*rig.calls.borrow(), vec!["HashPassword".to_string()]);
}

#[test]
fn filters_in_order_once() {
    let rig = rig(None, None, false);
    rig.hasher.hash(b"pw").unwrap();
    assert_eq!(
        *rig.calls.borrow(),
        vec!["algorithm".to_string(), "options".to_string()]
    );
    assert_eq!(
        *rig.filters.seen_algorithm_for_options.borrow(),
        Some(Value::Str("argon2id".into()))
    );
}

#[test]
fn filter_values_are_read_before_use() {
    // A bridge carries PHP values unconverted (Opaque) until Rust reads them.
    speedyspec_hooks::set_opaque_reader(speedyspec_hooks::OpaqueReader {
        readable: |any| any.is::<Value>(),
        read: |any| any.downcast_ref::<Value>().cloned(),
    });
    let carried = |v: Value| Value::Opaque(Rc::new(v));
    let rig = rig(
        Some(carried(Value::Str("2y".into()))),
        Some(carried(Value::Array(vec![(
            Key::Str("cost".into()),
            Value::Int(5),
        )]))),
        false,
    );
    let hash = text(rig.hasher.hash(b"pw").unwrap());
    assert!(hash.starts_with("$wp$2y$05$"), "{hash}");
}

#[test]
fn non_finite_or_out_of_range_float_algorithm_is_a_value_error() {
    // PHP's `string|int|null` parameter can't take these floats as integers, so it reads them as strings
    // ("NAN", "INF", "1.0E+20"), which name no algorithm.
    for f in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 1e20, -1e20] {
        let rig = rig(Some(Value::Float(f)), None, false);
        assert_eq!(
            rig.hasher.hash(b"pw"),
            Err(PasswordError::value_error(
                "password_hash(): Argument #2 ($algo) must be a valid password hashing algorithm"
            )),
            "{f}"
        );
    }
}

/// Warnings that fail as a throwing PHP error handler does.
struct ThrowingWarnings;

impl speedyspec_wp_password_contracts::Warnings for ThrowingWarnings {
    fn warn(&self, _message: &str) -> Result<(), PasswordError> {
        Err(PasswordError::Unwinding)
    }
}

#[test]
fn a_failing_warning_stops_the_hash() {
    let filters = Rc::new(FakeFilters::with(
        Some(Value::Str("2y".into())),
        Some(Value::Array(vec![(
            Key::Str("salt".into()),
            Value::Str("x".into()),
        )])),
        calls(),
    ));
    let bcrypt = Rc::new(FakeBcrypt::new());
    let hasher = DefaultPasswordHasher::new(Rc::new(PasswordPorts {
        filters,
        hasher_global: Rc::new(FakeHasher {
            set: false,
            calls: calls(),
        }),
        bcrypt: bcrypt.clone(),
        argon2: Rc::new(FakeArgon2::new()),
        random: Rc::new(FixedBytes(1)),
        warnings: Rc::new(ThrowingWarnings),
    }));
    assert_eq!(hasher.hash(b"pw"), Err(PasswordError::Unwinding));
}
