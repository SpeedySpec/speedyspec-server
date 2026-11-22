//! WordPress's rules for its sodium functions, through test doubles of the crypto ports.

use std::cell::RefCell;

use speedyspec_wp_crypto::*;
use speedyspec_wp_crypto_contracts::{
    Base64UrlNoPad, Clock, DetachedVerifier, GenericHash, SIGN_BYTES, SIGN_PUBLICKEY_BYTES,
    Translator,
};

/// Answers every hash with the message reversed and records the key and length it was asked for.
#[derive(Default)]
struct RecordingHash(RefCell<Vec<(Vec<u8>, usize)>>);

impl GenericHash for RecordingHash {
    fn hash(&self, message: &[u8], key: &[u8], out_len: usize) -> Vec<u8> {
        self.0.borrow_mut().push((key.to_vec(), out_len));
        message.iter().rev().copied().collect()
    }
}

/// Hex instead of base64, so expected values are easy to read.
struct Hex;

impl Base64UrlNoPad for Hex {
    fn encode(&self, bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }
}

/// Accepts one (signature, key) pair.
struct AcceptOne {
    signature: [u8; SIGN_BYTES],
    key: [u8; SIGN_PUBLICKEY_BYTES],
    calls: RefCell<u32>,
    messages: RefCell<Vec<Vec<u8>>>,
}

impl DetachedVerifier for AcceptOne {
    fn verify(
        &self,
        signature: &[u8; SIGN_BYTES],
        _message: &[u8],
        key: &[u8; SIGN_PUBLICKEY_BYTES],
    ) -> bool {
        *self.calls.borrow_mut() += 1;
        self.messages.borrow_mut().push(_message.to_vec());
        *signature == self.signature && *key == self.key
    }
}

struct At(i64);

impl Clock for At {
    fn now(&self) -> i64 {
        self.0
    }
}

struct Shout;

impl Translator for Shout {
    fn translate(&self, text: &str) -> String {
        text.to_uppercase().replace("%S", "%s")
    }
}

fn b64(bytes: &[u8]) -> Vec<u8> {
    const A: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = Vec::new();
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, b)| n | (u32::from(*b) << (16 - 8 * i)));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(A[((n >> (18 - 6 * i)) & 63) as usize]);
            } else {
                out.push(b'=');
            }
        }
    }
    out
}

fn verifier() -> AcceptOne {
    AcceptOne {
        signature: [7; SIGN_BYTES],
        key: [9; SIGN_PUBLICKEY_BYTES],
        calls: RefCell::new(0),
        messages: RefCell::default(),
    }
}

#[test]
fn fast_hash_prefixes_and_encodes() {
    let hash = RecordingHash::default();
    assert_eq!(wp_fast_hash(&hash, &Hex, b"ab"), "$generic$6261");
    assert_eq!(*hash.0.borrow(), vec![(b"wp_fast_hash_6.8+".to_vec(), 30)]);
}

#[test]
fn verify_accepts_its_own_hash() {
    let hash = RecordingHash::default();
    assert!(wp_verify_fast_hash(&hash, &Hex, b"ab", b"$generic$6261"));
}

#[test]
fn verify_rejects_unprefixed_and_phpass() {
    let hash = RecordingHash::default();
    assert!(!wp_verify_fast_hash(
        &hash,
        &Hex,
        b"ab",
        b"$P$BXq0ZdrFmZxbIQhbVaVZpPfCU1Kq5e."
    ));
    assert!(!wp_verify_fast_hash(&hash, &Hex, b"ab", b""));
    assert!(!wp_verify_fast_hash(&hash, &Hex, b"ab", b"6261"));
    assert!(
        hash.0.borrow().is_empty(),
        "no hash is computed for a hash without the prefix"
    );
}

#[test]
fn verify_rejects_partial_hash() {
    let hash = RecordingHash::default();
    assert!(!wp_verify_fast_hash(&hash, &Hex, b"ab", b"$generic$626"));
}

#[test]
fn trusted_keys_cut_off() {
    assert_eq!(
        default_trusted_keys(&At(1_617_235_199)),
        vec![WPORG_KEY_1.as_bytes().to_vec()]
    );
    assert!(default_trusted_keys(&At(1_617_235_200)).is_empty());
}

#[test]
fn php_base64_decode_matches_php() {
    // PHP 8.5.12's base64_decode() without $strict.
    let cases: &[(&[u8], &[u8])] = &[
        (b"QUJD", b"ABC"),
        (b"QUJ D\n", b"ABC"),
        (b"QUI=", b"AB"),
        (b"QUI", b"AB"),
        (b"Q", b""),
        (b"!!QUJD", b"ABC"),
        (b"QU=JD", b"ABC"),
        (b"QUJD=QUJD", b"ABCABC"),
        (b"QQ==QQ==", b"\x41\x04\x10"),
        (b"QQ", b"A"),
        (b"QUJDRA", b"ABCD"),
        (b"====", b""),
        (b"", b""),
        (b"QUJ\0D", b"ABC"),
        (b"QU JD==", b"ABC"),
        (b"Q=UJD", b"ABC"),
        (b"QUJDR===", b"ABC"),
    ];
    for (input, expected) in cases {
        assert_eq!(
            php_base64_decode(input),
            *expected,
            "{:?}",
            String::from_utf8_lossy(input)
        );
    }
}

#[test]
fn basename_matches_wordpress() {
    let cases: &[(&[u8], &[u8])] = &[
        (b"/a/b/c.zip", b"c.zip"),
        (b"c:\\x\\y.zip", b"y.zip"),
        (b"/a/b/", b"b"),
        ("/tmp/ü.zip".as_bytes(), "ü.zip".as_bytes()),
        (b"c.zip", b"c.zip"),
        (b"", b""),
        (b"/", b""),
        (b"a b+c.zip", b"a b+c.zip"),
        (b"/x/%41.zip", b"%41.zip"),
    ];
    for (path, expected) in cases {
        assert_eq!(
            wp_basename(path),
            *expected,
            "{:?}",
            String::from_utf8_lossy(path)
        );
    }
}

#[test]
fn falsy_signatures_are_no_signature() {
    let v = verifier();
    assert_eq!(
        check_signatures(&v, Some(&[0; 48]), None, &[b64(&[9; 32])]),
        SignatureOutcome::NoSignature
    );
    assert_eq!(*v.calls.borrow(), 0);
}

#[test]
fn loop_counts_skips() {
    let v = verifier();
    let signatures = [b64(&[1; 63]), b64(&[2; 64])];
    let keys = [b64(&[3; 31]), b64(&[4; 32])];
    match check_signatures(&v, Some(&[0; 48]), Some(&signatures), &keys) {
        SignatureOutcome::Failed {
            skipped_key,
            skipped_sig,
            ..
        } => {
            assert_eq!((skipped_key, skipped_sig), (1, 1));
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn short_key_counts_once_per_signature_tried() {
    let v = verifier();
    let signatures = [b64(&[2; 64]), b64(&[5; 64])];
    let keys = [b64(&[3; 31])];
    match check_signatures(&v, Some(&[0; 48]), Some(&signatures), &keys) {
        SignatureOutcome::Failed {
            skipped_key,
            skipped_sig,
            ..
        } => assert_eq!((skipped_key, skipped_sig), (2, 0)),
        other => panic!("{other:?}"),
    }
}

#[test]
fn loop_first_match_wins() {
    let v = verifier();
    let signatures = [b64(&[1; 64]), b64(&[7; 64]), b64(&[7; 64])];
    let keys = [b64(&[8; 32]), b64(&[9; 32])];
    assert_eq!(
        check_signatures(&v, Some(&[0; 48]), Some(&signatures), &keys),
        SignatureOutcome::Verified
    );
    assert_eq!(
        *v.calls.borrow(),
        4,
        "stops at the first signature and key that verify"
    );
}

#[test]
fn empty_keys_fail() {
    let v = verifier();
    let outcome = check_signatures(&v, Some(&[0xab; 48]), Some(&[b64(&[7; 64])]), &[]);
    assert_eq!(
        outcome,
        SignatureOutcome::Failed {
            hash_hex: "ab".repeat(48),
            skipped_key: 0,
            skipped_sig: 0
        }
    );
}

#[test]
fn empty_signature_list_is_attempted_and_fails() {
    // A non-empty PHP array holding nothing usable still enters the loop; only a falsy value is "no signature".
    let v = verifier();
    assert!(matches!(
        check_signatures(&v, Some(&[0; 48]), Some(&[]), &[]),
        SignatureOutcome::Failed { .. }
    ));
}

#[test]
fn hash_hex_is_lowercase_hex_of_sha384() {
    let path =
        std::env::temp_dir().join(format!("speedyspec-wp-crypto-{}.empty", std::process::id()));
    std::fs::write(&path, b"").unwrap();
    let digest = sha384_file(&path).expect("readable");
    std::fs::remove_file(&path).unwrap();
    assert_eq!(
        hex(&digest),
        "38b060a751ac96384cd9327eb1b1e36a21fdb71114be07434c0cc7bf63f6e1da274edebfe76f65fbd51ad2f14898b95b"
    );
}

#[test]
fn sha384_file_missing_is_none() {
    assert!(sha384_file(std::path::Path::new("/nonexistent/speedyspec/file.zip")).is_none());
    assert!(
        sha384_file(&std::env::temp_dir()).is_none(),
        "a directory is not a file"
    );
}

#[test]
fn message_translates_then_substitutes() {
    let no_signature = signature_message(&Shout, SignatureMessage::NoSignature, || {
        Ok::<_, ()>("a&amp;b.zip".into())
    });
    assert_eq!(
        no_signature.unwrap(),
        "THE AUTHENTICITY OF <span class=\"code\">a&amp;b.zip</span> COULD NOT BE VERIFIED AS NO SIGNATURE WAS FOUND."
    );
    let failed = signature_message(&Shout, SignatureMessage::Failed, || {
        Ok::<_, ()>("f.zip".into())
    });
    assert_eq!(
        failed.unwrap(),
        "THE AUTHENTICITY OF <span class=\"code\">f.zip</span> COULD NOT BE VERIFIED."
    );
}

#[test]
fn message_translates_before_escaping_and_only_once() {
    // sprintf( __( ... ), '<span class="code">' . esc_html( $name ) . '</span>' ): PHP evaluates __() first.
    struct Logging<'a>(&'a RefCell<Vec<&'static str>>);
    impl Translator for Logging<'_> {
        fn translate(&self, text: &str) -> String {
            self.0.borrow_mut().push("translate");
            text.to_string()
        }
    }
    let log = RefCell::new(Vec::new());
    let message = signature_message(&Logging(&log), SignatureMessage::Failed, || {
        log.borrow_mut().push("escape");
        Ok::<_, ()>("f.zip".into())
    });
    assert!(message.is_ok());
    assert_eq!(*log.borrow(), vec!["translate", "escape"]);
}

#[test]
fn escape_failure_is_the_message_failure() {
    let message = signature_message(&Shout, SignatureMessage::Failed, || {
        Err::<String, _>("thrown")
    });
    assert_eq!(message, Err("thrown"));
}

#[test]
fn message_takes_numbered_placeholders_and_percent() {
    struct Numbered;
    impl Translator for Numbered {
        fn translate(&self, _text: &str) -> String {
            "100%% sure: %1$s".to_string()
        }
    }
    let message = signature_message(&Numbered, SignatureMessage::Failed, || {
        Ok::<_, ()>("f.zip".into())
    });
    assert_eq!(
        message.unwrap(),
        "100% sure: <span class=\"code\">f.zip</span>"
    );
}

#[test]
fn keys_are_read_only_when_reached() {
    // WordPress decodes a key only inside the loop, after a signature of the right length, and stops at the first
    // match: a key it never reaches can't fail the call.
    let v = verifier();
    let reached = RefCell::new(Vec::new());
    let keys = [Ok(b64(&[9; 32])), Err("an array")];
    let outcome = check_signatures_with(
        &v,
        Some(&[0; 48]),
        Some(&[b64(&[7; 64])]),
        &keys,
        |s: &Vec<u8>| Ok(s.clone()),
        |k: &Result<Vec<u8>, &'static str>| {
            reached.borrow_mut().push(k.is_ok());
            k.clone()
        },
    );
    assert_eq!(outcome, Ok(SignatureOutcome::Verified));
    assert_eq!(*reached.borrow(), vec![true]);
}

#[test]
fn keys_after_short_signatures_are_never_read() {
    let v = verifier();
    let outcome = check_signatures_with(
        &v,
        Some(&[0; 48]),
        Some(&[b64(&[7; 63])]),
        &[()],
        |s: &Vec<u8>| Ok::<_, &str>(s.clone()),
        |_: &()| Err("never read"),
    );
    assert!(matches!(
        outcome,
        Ok(SignatureOutcome::Failed {
            skipped_sig: 1,
            skipped_key: 0,
            ..
        })
    ));
}

#[test]
fn signatures_are_read_one_at_a_time_and_failures_stop_the_check() {
    let v = verifier();
    let signatures = [Ok(b64(&[7; 64])), Err("an array")];
    let verified = check_signatures_with(
        &v,
        Some(&[0; 48]),
        Some(&signatures),
        &[b64(&[9; 32])],
        |s: &Result<Vec<u8>, &'static str>| s.clone(),
        |k: &Vec<u8>| Ok(k.clone()),
    );
    assert_eq!(
        verified,
        Ok(SignatureOutcome::Verified),
        "a signature after the match is never read"
    );
    let failing = [Err("an array"), Ok(b64(&[7; 64]))];
    let failed = check_signatures_with(
        &v,
        Some(&[0; 48]),
        Some(&failing),
        &[b64(&[9; 32])],
        |s: &Result<Vec<u8>, &'static str>| s.clone(),
        |k: &Vec<u8>| Ok(k.clone()),
    );
    assert_eq!(failed, Err("an array"));
}

#[test]
fn unreadable_file_verifies_against_the_empty_message() {
    // hash_file() returns false, which WordPress passes on as the message ("") and as bin2hex()'s input.
    let v = verifier();
    let outcome = check_signatures(&v, None, Some(&[b64(&[1; 64])]), &[b64(&[2; 32])]);
    assert_eq!(
        outcome,
        SignatureOutcome::Failed {
            hash_hex: String::new(),
            skipped_key: 0,
            skipped_sig: 0
        }
    );
    assert_eq!(*v.messages.borrow(), vec![Vec::<u8>::new()]);
}
