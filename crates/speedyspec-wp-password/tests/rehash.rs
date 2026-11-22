//! `wp_password_needs_rehash()` and `password_needs_rehash()`, replayed against what stock WordPress and PHP answered.

mod common;

use std::rc::Rc;

use common::*;
use speedyspec_hooks::Value;
use speedyspec_wp_password::{DefaultRehashPolicy, PasswordPorts, password_needs_rehash};
use speedyspec_wp_password_bcrypt::BcryptCrate;
use speedyspec_wp_password_contracts::{PasswordError, RehashPolicy};

fn policy(
    algorithm: Option<Value>,
    options: Option<Value>,
    hasher_set: bool,
) -> (
    DefaultRehashPolicy,
    Rc<FakeFilters>,
    Rc<std::cell::RefCell<Vec<String>>>,
) {
    let calls = calls();
    let filters = Rc::new(FakeFilters::with(algorithm, options, calls.clone()));
    let ports = Rc::new(PasswordPorts {
        filters: filters.clone(),
        hasher_global: Rc::new(FakeHasher {
            set: hasher_set,
            calls: calls.clone(),
        }),
        bcrypt: Rc::new(BcryptCrate),
        argon2: Rc::new(FakeArgon2::new()),
        random: Rc::new(FixedBytes(1)),
        warnings: Rc::new(CollectWarnings(Default::default())),
    });
    (DefaultRehashPolicy::new(ports), filters, calls)
}

fn same(name: &str, what: &str, got: Result<bool, PasswordError>, recorded: &serde_json::Value) {
    match (got, recorded.get("exception")) {
        (Ok(b), None) => assert_eq!(b, recorded["result"].as_bool().unwrap(), "{name} ({what})"),
        (Err(PasswordError::Throw { class, message }), Some(e)) => assert_eq!(
            (class.as_str(), message.as_str()),
            (e["class"].as_str().unwrap(), e["message"].as_str().unwrap()),
            "{name} ({what})"
        ),
        (got, _) => panic!("{name} ({what}): {got:?} vs {recorded}"),
    }
}

#[test]
fn rehash_replays_recorded() {
    let cases = cases();
    let all = cases["needs_rehash"].as_array().unwrap();
    assert!(all.len() > 150);
    for case in all {
        let name = case["name"].as_str().unwrap();
        let hash = case["hash"].as_str().unwrap().as_bytes();
        let algorithm = value(&case["algorithm"]["value"]);
        let options = value(&case["options"]);
        let unprefixed = hash.strip_prefix(b"$wp").unwrap_or(hash);
        same(
            name,
            "php",
            password_needs_rehash(unprefixed, &algorithm, &options),
            &case["php"],
        );
        let (policy, _, _) = policy(Some(algorithm), Some(options), false);
        let wp = policy
            .needs_rehash(hash, &Value::Str(String::new()))
            .map(|v| v == Value::Bool(true));
        same(name, "wp", wp, &case["wp"]);
    }
}

#[test]
fn bcrypt_needs_rehash_under_argon2id_default() {
    let (policy, _, _) = policy(None, None, false);
    let user = Value::Str(String::new());
    let wp = format!("$wp$2y$12${}", ".".repeat(53));
    assert_eq!(
        policy.needs_rehash(wp.as_bytes(), &user).unwrap(),
        Value::Bool(true)
    );
    assert_eq!(
        policy.needs_rehash(&wp.as_bytes()[3..], &user).unwrap(),
        Value::Bool(true)
    );
    let fresh = "$argon2id$v=19$m=65536,t=4,p=1$AQEBAQEBAQEBAQEBAQEBAQ$AQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQE";
    assert_eq!(
        policy.needs_rehash(fresh.as_bytes(), &user).unwrap(),
        Value::Bool(false)
    );
}

#[test]
fn wp_hasher_set_means_no_rehash_and_no_filters() {
    let (policy, _, calls) = policy(None, None, true);
    assert_eq!(
        policy.needs_rehash(b"$P$xxxx", &Value::Int(1)).unwrap(),
        Value::Bool(false)
    );
    assert!(calls.borrow().is_empty());
}

#[test]
fn filter_receives_hash_and_user_id() {
    let (policy, filters, calls) = policy(None, None, false);
    policy.needs_rehash(b"$2y$05$abc", &Value::Int(9)).unwrap();
    assert_eq!(
        *calls.borrow(),
        vec!["algorithm", "options", "password_needs_rehash"]
    );
    assert_eq!(
        filters.rehash_args.borrow().clone().unwrap(),
        (true, b"$2y$05$abc".to_vec(), Value::Int(9))
    );
}
