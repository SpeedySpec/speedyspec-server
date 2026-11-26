//! Random kses input against the pinned WordPress release: every difference must pass the gate rule and the port's
//! output must hold no exploit. `KSES_FUZZ_CASES` raises the case count. Failing inputs are saved under the testkit's
//! corpus/fuzz-found/ so they become regression inputs.

mod common;

use std::cell::RefCell;
use std::time::Duration;

use proptest::prelude::*;
use proptest::test_runner::{Config, TestRunner};
use serde_json::Value as Json;
use sha2::{Digest, Sha256};
use speedyspec_wp_kses_contracts::{AllowedSpec, Context};
use speedyspec_wp_kses_testkit::{
    PHP_TIMED_OUT, PhpOracle, Verdict, bytes, from_bytes, gate, token_soup,
};

fn cases() -> u32 {
    std::env::var("KSES_FUZZ_CASES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(2000)
}

/// PHP's answer, or `None` when PHP never finishes (a `technical` break: the port must still finish, safely).
fn php_kses(php: &mut PhpOracle, input: &[u8], ctx: &str) -> Option<Vec<u8>> {
    match php.call("wp_kses", vec![bytes(input), Json::String(ctx.into())]) {
        Ok(r) => Some(from_bytes(&r.returned)),
        Err(e) if e == PHP_TIMED_OUT => None,
        Err(e) => panic!("{e}"),
    }
}

fn oracle() -> PhpOracle {
    let mut php = PhpOracle::start().unwrap();
    php.set_timeout(Duration::from_secs(10));
    php
}

fn save(input: &[u8]) {
    let dir = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../speedyspec-wp-kses-testkit/corpus/fuzz-found"
    );
    let _ = std::fs::write(format!("{dir}/{:x}.bin", Sha256::digest(input)), input);
}

#[test]
fn random_input_matches_php_and_stays_safe() {
    let php = RefCell::new(oracle());
    let mut runner = TestRunner::new(Config {
        cases: cases(),
        failure_persistence: None,
        ..Config::default()
    });
    let current = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../target/kses-fuzz-current.bin"
    );
    let result = runner.run(&token_soup(), |input| {
        // A PHP hang leaves the input that caused it here.
        let _ = std::fs::write(current, &input);
        for ctx in ["post", "data", "strip"] {
            let (_, api) = common::setup();
            let expected = php_kses(&mut php.borrow_mut(), &input, ctx);
            let ours = api
                .kses(
                    &input,
                    &AllowedSpec::Context(Context::from_bytes(ctx.as_bytes())),
                    None,
                )
                .unwrap();
            let finder = common::exploit_finder_for(&api, ctx);
            if let Some(exploit) = finder(&ours) {
                save(&input);
                prop_assert!(
                    false,
                    "{ctx}: exploit in the port's output: {exploit}\n{}",
                    common::short(&input)
                );
            }
            let Some(expected) = expected else {
                // PHP never finishes (kses:never-settling-token in compat/kses/breaks.json); the port finished and
                // its output is exploit-free. Not saved: corpus/fuzz-found/ already holds a minimal case.
                continue;
            };
            if let Verdict::Fail(why) = gate(&expected, &ours, Some(&*finder)) {
                save(&input);
                prop_assert!(false, "{ctx}: {why}\ninput: {}", common::short(&input));
            }
        }
        Ok(())
    });
    if let Err(e) = result {
        panic!("{e}");
    }
}

#[test]
fn saved_fuzz_findings_still_match() {
    let dir = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../speedyspec-wp-kses-testkit/corpus/fuzz-found"
    );
    let mut php = oracle();
    for entry in std::fs::read_dir(dir).unwrap().flatten() {
        let input = std::fs::read(entry.path()).unwrap();
        for ctx in ["post", "data", "strip"] {
            let (_, api) = common::setup();
            let ours = api
                .kses(
                    &input,
                    &AllowedSpec::Context(Context::from_bytes(ctx.as_bytes())),
                    None,
                )
                .unwrap();
            let finder = common::exploit_finder_for(&api, ctx);
            assert_eq!(finder(&ours), None, "{ctx} {}", entry.path().display());
            // `None`: PHP never finishes on this input (compat/kses/breaks.json, kses:never-settling-token).
            if let Some(expected) = php_kses(&mut php, &input, ctx) {
                assert_eq!(ours, expected, "{ctx} {}", entry.path().display());
            }
        }
    }
}
