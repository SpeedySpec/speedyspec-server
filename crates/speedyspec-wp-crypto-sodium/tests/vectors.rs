//! The crypto ports over libsodium, against known answers: RFC 8032's Ed25519 tests, BLAKE2b's keyed test vector, and
//! values recorded from the static PHP's ext-sodium.

use speedyspec_wp_crypto_contracts::{
    Base64UrlNoPad, CryptoLibrary, DetachedVerifier, GenericHash,
};
use speedyspec_wp_crypto_sodium::Sodium;

fn unhex(text: &str) -> Vec<u8> {
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).unwrap())
        .collect()
}

fn sodium() -> Sodium {
    Sodium::init().expect("libsodium initializes")
}

#[test]
fn blake2b_keyed_vector() {
    let key: Vec<u8> = (0..64).collect();
    assert_eq!(
        sodium().hash(b"", &key, 64),
        unhex(
            "10ebb67700b1868efb4417987acf4690ae9d972fb7a590c2f02871799aaa4786\
             b5e996e8f0f4eb981fc214b005f42d2ff4233499391653df7aefcbc13fc51568"
        )
    );
}

#[test]
fn blake2b_matches_php_fast_hash_key() {
    // bin2hex(sodium_crypto_generichash('abc', 'wp_fast_hash_6.8+', 30)) on the static PHP.
    assert_eq!(
        sodium().hash(b"abc", b"wp_fast_hash_6.8+", 30),
        unhex("61e68faff8adc6750ec850419b5e841859edfe7ceaa5b2fbdbc71a909e14")
    );
}

#[test]
fn ed25519_rfc8032_test_1_and_2() {
    let s = sodium();
    let cases = [
        (
            "d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a",
            "",
            "e5564300c360ac729086e2cc806e828a84877f1eb8e5d974d873e065224901555fb8821590a33bacc61e39701cf9b46bd25bf5f0595bbe24655141438e7a100b",
        ),
        (
            "3d4017c3e843895a92b70aa74d1b7ebc9c982ccf2ec4968cc0cd55f12af4660c",
            "72",
            "92a009a9f0d4cab8720e820b5f642540a2b27b5416503f8fb3762223ebdb69da085ac1e43e15996e458f3613d0f11d8c387b2eaeb4302aeeb00d291612bb0c00",
        ),
    ];
    for (key, message, signature) in cases {
        let key: [u8; 32] = unhex(key).try_into().unwrap();
        let mut signature: [u8; 64] = unhex(signature).try_into().unwrap();
        let message = unhex(message);
        assert!(s.verify(&signature, &message, &key));
        signature[0] ^= 1;
        assert!(!s.verify(&signature, &message, &key));
    }
}

#[test]
fn base64_urlsafe_no_padding() {
    assert_eq!(sodium().encode(&[0xfb, 0xff]), "-_8");
    assert_eq!(sodium().encode(b""), "");
}

#[test]
fn version_is_pinned() {
    assert_eq!(sodium().version(), "1.0.22");
}
