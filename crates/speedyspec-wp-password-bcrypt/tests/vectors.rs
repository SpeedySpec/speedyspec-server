//! The bcrypt adapter against bcrypt recorded from the static PHP's `crypt()` and `password_verify()`
//! (`compat/password/cases.json`, section `bcrypt`).

use std::path::PathBuf;

use serde_json::Value;
use speedyspec_wp_password_bcrypt::BcryptCrate;
use speedyspec_wp_password_contracts::Bcrypt;

fn cases() -> Vec<Value> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../compat/password/cases.json");
    let all: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    all["bcrypt"].as_array().unwrap().clone()
}

fn unhex(text: &str) -> Vec<u8> {
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).unwrap())
        .collect()
}

/// bcrypt's own base64 (`./A-Za-z0-9`), for the 22-character salt of a setting.
fn bcrypt_salt(text: &str) -> [u8; 16] {
    const ALPHABET: &[u8] = b"./ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    let mut bits = 0u32;
    let mut count = 0;
    let mut out = Vec::new();
    for c in text.bytes() {
        bits = (bits << 6) | ALPHABET.iter().position(|a| *a == c).unwrap() as u32;
        count += 6;
        if count >= 8 {
            count -= 8;
            out.push((bits >> count) as u8);
        }
    }
    out.truncate(16);
    out.try_into().unwrap()
}

fn prefix(case: &Value) -> &str {
    &case["setting"].as_str().unwrap()[..4]
}

#[test]
fn hash_matches_php_crypt() {
    let cases: Vec<Value> = cases()
        .into_iter()
        .filter(|c| prefix(c) == "$2y$")
        .collect();
    assert!(!cases.is_empty());
    for case in cases {
        let name = case["name"].as_str().unwrap();
        let setting = case["setting"].as_str().unwrap();
        let cost: u32 = setting[4..6].parse().unwrap();
        let hash = BcryptCrate
            .hash(
                &unhex(case["password"].as_str().unwrap()),
                cost,
                &bcrypt_salt(&setting[7..]),
            )
            .unwrap();
        assert_eq!(hash, case["result"].as_str().unwrap(), "{name}");
    }
}

#[test]
fn verifies_php_vectors_all_variants() {
    for case in cases().into_iter().filter(|c| prefix(c) != "$2x$") {
        let name = case["name"].as_str().unwrap();
        let password = unhex(case["password"].as_str().unwrap());
        let hash = case["result"].as_str().unwrap().as_bytes();
        let mut wrong = password.clone();
        wrong.push(b'x');
        assert_eq!(
            BcryptCrate.verify(&password, hash),
            case["verify_right"]["result"].as_bool().unwrap(),
            "{name}: right password"
        );
        assert_eq!(
            BcryptCrate.verify(&wrong, hash),
            case["verify_wrong"]["result"].as_bool().unwrap(),
            "{name}: wrong password"
        );
    }
}

#[test]
fn refuses_2x() {
    for case in cases().into_iter().filter(|c| prefix(c) == "$2x$") {
        let password = unhex(case["password"].as_str().unwrap());
        assert!(
            !BcryptCrate.verify(&password, case["result"].as_str().unwrap().as_bytes()),
            "{}",
            case["name"]
        );
    }
}

#[test]
fn refuses_garbage() {
    for hash in [
        &b""[..],
        b"$2y$",
        &[b'a'; 59],
        b"$2y$05$!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!",
        &[0xff; 60],
    ] {
        assert!(!BcryptCrate.verify(b"password", hash));
    }
}
