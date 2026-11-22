//! PHP's ext-sodium and the Rust adapter linked into one binary run one libsodium: a second copy would fail the link
//! with duplicate symbols, and a different one would report a different version.

use std::rc::Rc;

use speedyspec_hooks::Value;
use speedyspec_hooks_memory::MemoryHooks;
use speedyspec_php::{BootOptions, Php};
use speedyspec_wp_crypto_contracts::{CryptoLibrary, GenericHash};
use speedyspec_wp_crypto_sodium::Sodium;

fn main() {
    let php = Php::boot(BootOptions {
        hooks: Rc::new(MemoryHooks::new()),
        preload: &[],
        areas: Vec::new(),
    })
    .expect("PHP boots");
    let sodium = Sodium::init().expect("libsodium initializes");

    let php_version = php
        .eval("version", "return SODIUM_LIBRARY_VERSION;")
        .expect("the probe runs");
    assert_eq!(
        php_version,
        Value::Str(sodium.version()),
        "PHP and Rust report the same libsodium"
    );

    let php_hash = php
        .eval(
            "hash",
            "return bin2hex(sodium_crypto_generichash('abc', 'wp_fast_hash_6.8+', 30));",
        )
        .expect("the probe runs");
    let rust_hash: String = sodium
        .hash(b"abc", b"wp_fast_hash_6.8+", 30)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    assert_eq!(php_hash, Value::Str(rust_hash), "PHP and Rust hash alike");
    println!("one libsodium: {}", sodium.version());
}
