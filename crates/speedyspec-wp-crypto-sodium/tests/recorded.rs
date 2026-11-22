//! Replays what `compat/crypto/record.php` recorded from WordPress 7.1.5 on PHP 8.5.12, through WordPress's rules
//! (`speedyspec-wp-crypto`) over libsodium.
//!
//! Fast hashes must come out byte for byte, and every signature case must end the same way, with the same hash, skip
//! counts and message. The filter's raw value and the error data's pass-through fields are the PHP bridge's, and are
//! replayed there. Two differences are intended and listed in [`DIVERGENCES`].

use std::path::PathBuf;

use base64::Engine as _;
use serde_json::Value;
use speedyspec_wp_crypto::{
    SignatureMessage, SignatureOutcome, check_signatures, sha384_file, signature_message,
    wp_basename, wp_fast_hash, wp_verify_fast_hash,
};
use speedyspec_wp_crypto_contracts::Translator;
use speedyspec_wp_crypto_sodium::Sodium;

/// Cases where SpeedySpec answers differently from WordPress on purpose.
///
/// `verify/phpass`: SpeedySpec carries no phpass, so a pre-6.8 phpass hash never verifies (Jacob, 2026-11-21).
const DIVERGENCES: &[&str] = &["verify/phpass"];

fn compat() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../compat/crypto")
}

fn cases() -> Value {
    serde_json::from_str(&std::fs::read_to_string(compat().join("cases.json")).unwrap()).unwrap()
}

fn b64(value: &Value) -> Vec<u8> {
    base64::engine::general_purpose::STANDARD
        .decode(value.as_str().unwrap())
        .unwrap()
}

struct Untranslated;

impl Translator for Untranslated {
    type Error = ();

    fn translate(&self, text: &str) -> Result<String, ()> {
        Ok(text.to_string())
    }
}

/// `esc_html()` for the recorded names, which are plain ASCII with no entities.
fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#039;")
}

/// A PHP value as WordPress's string coercion sees it: `(array)` for lists, `(string)` for scalars.
fn strings(value: &Value) -> Vec<Vec<u8>> {
    let scalar = |v: &Value| match v {
        Value::String(s) => s.as_bytes().to_vec(),
        Value::Number(n) => n.to_string().into_bytes(),
        Value::Bool(true) => b"1".to_vec(),
        _ => Vec::new(),
    };
    match value {
        Value::Null => Vec::new(),
        Value::Array(items) => items.iter().map(scalar).collect(),
        Value::Object(items) => items.values().map(scalar).collect(),
        other => vec![scalar(other)],
    }
}

/// PHP's `! $value`.
fn falsy(value: &Value) -> bool {
    match value {
        Value::Null | Value::Bool(false) => true,
        Value::String(s) => s.is_empty() || s == "0",
        Value::Array(items) => items.is_empty(),
        Value::Object(items) => items.is_empty(),
        Value::Number(n) => n.as_f64() == Some(0.0),
        Value::Bool(true) => false,
    }
}

#[test]
fn fast_hashes_match() {
    let sodium = Sodium::init().unwrap();
    for case in cases()["fast_hash"].as_array().unwrap() {
        assert_eq!(
            wp_fast_hash(&sodium, &sodium, &b64(&case["message_b64"])),
            case["hash"].as_str().unwrap()
        );
    }
}

#[test]
fn verifications_match() {
    let sodium = Sodium::init().unwrap();
    for case in cases()["verify"].as_array().unwrap() {
        let name = format!("verify/{}", case["name"].as_str().unwrap());
        let got = wp_verify_fast_hash(
            &sodium,
            &sodium,
            &b64(&case["message_b64"]),
            case["hash"].as_str().unwrap().as_bytes(),
        );
        let recorded = case["result"].as_bool().unwrap();
        if DIVERGENCES.contains(&name.as_str()) {
            assert!(
                recorded && !got,
                "{name}: WordPress verifies it, SpeedySpec must not"
            );
        } else {
            assert_eq!(got, recorded, "{name}");
        }
    }
}

#[test]
fn signatures_match() {
    let sodium = Sodium::init().unwrap();
    let cases = cases();
    let signature_cases = cases["signature"].as_array().unwrap();
    assert!(signature_cases.len() >= 26);
    for case in signature_cases {
        let name = case["name"].as_str().unwrap();
        let path = compat().join("files").join(case["file"].as_str().unwrap());
        let filename = match &case["filename_for_errors"] {
            Value::String(s) => s.clone(),
            _ => String::from_utf8(wp_basename(path.to_str().unwrap().as_bytes())).unwrap(),
        };
        let signatures = (!falsy(&case["signatures"])).then(|| strings(&case["signatures"]));
        let hash = sha384_file(&path);
        let outcome = check_signatures(
            &sodium,
            hash.as_ref(),
            signatures.as_deref(),
            &strings(&case["filter"]),
        );
        let message = |which| {
            signature_message(&Untranslated, which, || Ok::<_, ()>(escape(&filename))).unwrap()
        };
        let recorded = &case["result"];
        match outcome {
            SignatureOutcome::Verified => assert_eq!(recorded, &Value::Bool(true), "{name}"),
            SignatureOutcome::NoSignature => {
                assert_eq!(
                    recorded["code"], "signature_verification_no_signature",
                    "{name}"
                );
                assert_eq!(
                    recorded["message"],
                    message(SignatureMessage::NoSignature),
                    "{name}"
                );
                assert_eq!(recorded["data"]["filename"], filename, "{name}");
            }
            SignatureOutcome::Failed {
                hash_hex,
                skipped_key,
                skipped_sig,
            } => {
                assert_eq!(recorded["code"], "signature_verification_failed", "{name}");
                assert_eq!(
                    recorded["message"],
                    message(SignatureMessage::Failed),
                    "{name}"
                );
                let data = &recorded["data"];
                assert_eq!(data["filename"], filename, "{name}");
                assert_eq!(data["hash"], hash_hex, "{name}");
                assert_eq!(data["skipped_key"], skipped_key, "{name}");
                assert_eq!(data["skipped_sig"], skipped_sig, "{name}");
            }
        }
    }
}
