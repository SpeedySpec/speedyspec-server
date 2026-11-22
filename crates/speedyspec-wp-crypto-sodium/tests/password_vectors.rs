//! The argon2 and random-bytes ports over libsodium, against raw argon2 recorded from the static PHP's
//! `sodium_crypto_pwhash()` (`compat/password/cases.json`, section `argon2_raw`).

use std::path::PathBuf;

use serde_json::Value;
use speedyspec_wp_crypto_contracts::{Argon2, Argon2Variant, RandomBytes};
use speedyspec_wp_crypto_sodium::Sodium;

fn sodium() -> Sodium {
    Sodium::init().expect("libsodium initializes")
}

fn cases() -> Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../compat/password/cases.json");
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn unhex(text: &str) -> Vec<u8> {
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).unwrap())
        .collect()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[test]
fn argon2_matches_php_sodium_crypto_pwhash() {
    let sodium = sodium();
    let cases = cases();
    let raw = cases["argon2_raw"].as_array().unwrap();
    assert!(!raw.is_empty());
    for case in raw {
        let name = case["name"].as_str().unwrap();
        let variant = match case["variant"].as_str().unwrap() {
            "argon2i" => Argon2Variant::Argon2i,
            _ => Argon2Variant::Argon2id,
        };
        let salt: [u8; 16] = unhex(case["salt_hex"].as_str().unwrap())
            .try_into()
            .unwrap();
        let derived = sodium.derive(
            variant,
            &unhex(case["password"].as_str().unwrap()),
            &salt,
            case["time_cost"].as_u64().unwrap() as u32,
            case["memory_kib"].as_u64().unwrap() as u32,
            32,
        );
        match case["result"].as_str() {
            Some(expected) => {
                assert_eq!(derived.map(|d| hex(&d)).as_deref(), Ok(expected), "{name}")
            }
            None => assert!(derived.is_err(), "{name}: PHP failed, so must the port"),
        }
    }
}

#[test]
fn argon2_rejects_below_libsodium_minimums() {
    let salt = [1u8; 16];
    assert!(
        sodium()
            .derive(Argon2Variant::Argon2i, b"pw", &salt, 2, 4096, 32)
            .is_err()
    );
}

#[test]
fn random_bytes_fill_differs() {
    let sodium = sodium();
    let (mut a, mut b) = ([0u8; 32], [0u8; 32]);
    sodium.fill(&mut a);
    sodium.fill(&mut b);
    assert_ne!(a, b);
    assert_ne!(a, [0u8; 32]);
}
