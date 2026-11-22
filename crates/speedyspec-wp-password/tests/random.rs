//! `wp_rand()` and `wp_generate_password()`.

mod common;

use std::cell::RefCell;
use std::rc::Rc;

use common::*;
use speedyspec_hooks::Value;
use speedyspec_wp_crypto_contracts::RandomBytes;
use speedyspec_wp_password::{
    CHARS, DefaultPasswordGenerator, DefaultRandomNumber, EXTRA_SPECIAL, SPECIAL, uniform,
};
use speedyspec_wp_password_contracts::{PasswordError, PasswordGenerator, RandomNumber};

/// Bytes from a script, then zeros.
struct Script(RefCell<Vec<u8>>);

impl RandomBytes for Script {
    fn fill(&self, out: &mut [u8]) {
        for b in out {
            let mut script = self.0.borrow_mut();
            *b = if script.is_empty() {
                0
            } else {
                script.remove(0)
            };
        }
    }
}

fn int(value: Value) -> i64 {
    match value {
        Value::Int(n) => n,
        other => panic!("not an int: {other:?}"),
    }
}

#[test]
fn uniform_stays_in_range() {
    let bytes = Script(RefCell::new((0..=255).cycle().take(4096).collect()));
    for _ in 0..200 {
        let n = uniform(&bytes, 3, 9);
        assert!((3..=9).contains(&n), "{n}");
    }
}

#[test]
fn uniform_rejects_biased_draws() {
    // Range of 3: u64::MAX draws fall in the biased tail and are drawn again; the next draw (7) gives 7 % 3 = 1.
    let mut script = vec![0xff; 8];
    script.extend(7u64.to_le_bytes());
    let bytes = Script(RefCell::new(script));
    assert_eq!(uniform(&bytes, 10, 12), 11);
}

#[test]
fn rand_zero_zero_is_zero() {
    let numbers = DefaultRandomNumber::new(Rc::new(FixedBytes(0xab)));
    assert_eq!(numbers.rand(0, 0).unwrap(), Value::Int(0));
}

#[test]
fn rand_swaps_reversed() {
    let numbers = DefaultRandomNumber::new(Rc::new(FixedBytes(0x5a)));
    for _ in 0..50 {
        let n = int(numbers.rand(10, 1).unwrap());
        assert!((1..=10).contains(&n));
    }
}

#[test]
fn rand_full_i64_range_and_reversed() {
    let numbers = DefaultRandomNumber::new(Rc::new(FixedBytes(0xff)));
    numbers.rand(i64::MIN, i64::MAX).unwrap();
    numbers.rand(i64::MAX, i64::MIN).unwrap();
    // absint(PHP_INT_MIN) is a float in PHP.
    let zeros = DefaultRandomNumber::new(Rc::new(FixedBytes(0)));
    assert_eq!(
        zeros.rand(i64::MIN, i64::MAX).unwrap(),
        Value::Float(9_223_372_036_854_775_808.0)
    );
}

#[test]
fn rand_replays_recorded_ranges() {
    let numbers = DefaultRandomNumber::new(Rc::new(speedyspec_wp_password_test_bytes()));
    for case in cases()["rand"].as_array().unwrap() {
        // The facade's casts: null → 0 / 4294967295, (int) of strings and floats.
        let cast = |v: &serde_json::Value, default: i64| match v {
            serde_json::Value::Null => default,
            serde_json::Value::String(s) => s.parse().unwrap(),
            other => other.as_f64().unwrap() as i64,
        };
        let (min, max) = (cast(&case["min"], 0), cast(&case["max"], 4_294_967_295));
        let (low, high) = (
            case["lowest"].as_i64().unwrap(),
            case["highest"].as_i64().unwrap(),
        );
        let (lo, hi) = (
            min.min(max).abs().min(min.max(max).abs()),
            min.min(max).abs().max(min.max(max).abs()),
        );
        assert!(
            lo <= low && high <= hi,
            "{}: PHP drew outside the rule",
            case["name"]
        );
        for _ in 0..50 {
            let n = int(numbers.rand(min, max).unwrap());
            assert!((lo..=hi).contains(&n), "{}: {n}", case["name"]);
        }
    }
}

/// Varied bytes, so draws spread.
fn speedyspec_wp_password_test_bytes() -> Script {
    Script(RefCell::new(
        (0..20000u32)
            .map(|i| (i.wrapping_mul(2_654_435_761) >> 13) as u8)
            .collect(),
    ))
}

/// A `wp_rand()` answering a script, recording its arguments.
struct ScriptedNumbers {
    answers: RefCell<Vec<i64>>,
    seen: RefCell<Vec<(i64, i64)>>,
}

impl RandomNumber for ScriptedNumbers {
    fn rand(&self, min: i64, max: i64) -> Result<Value, PasswordError> {
        self.seen.borrow_mut().push((min, max));
        Ok(Value::Int(self.answers.borrow_mut().remove(0)))
    }
}

fn generator(numbers: Rc<ScriptedNumbers>) -> (DefaultPasswordGenerator, Rc<FakeFilters>) {
    let filters = Rc::new(FakeFilters::new(calls()));
    let resolve = Rc::new(move || Ok(numbers.clone() as Rc<dyn RandomNumber>));
    (
        DefaultPasswordGenerator::new(resolve, filters.clone()),
        filters,
    )
}

#[test]
fn generate_uses_bound_random_number() {
    for case in cases()["generate"].as_array().unwrap() {
        let (special, extra) = (
            case["special"].as_bool().unwrap(),
            case["extra"].as_bool().unwrap(),
        );
        let mut chars = CHARS.to_string();
        if special {
            chars.push_str(SPECIAL);
        }
        if extra {
            chars.push_str(EXTRA_SPECIAL);
        }
        let mut sorted: Vec<char> = chars.chars().collect();
        sorted.sort();
        assert_eq!(
            sorted.into_iter().collect::<String>(),
            case["chars"].as_str().unwrap(),
            "{}",
            case["name"]
        );
        let numbers = Rc::new(ScriptedNumbers {
            answers: RefCell::new(vec![0, 1]),
            seen: RefCell::new(vec![]),
        });
        let (generator, _) = generator(numbers.clone());
        let password = generator
            .generate(&Value::Int(2), &Value::Bool(special), &Value::Bool(extra))
            .unwrap();
        assert_eq!(password, Value::Str("ab".into()));
        assert_eq!(*numbers.seen.borrow(), vec![(0, chars.len() as i64 - 1); 2]);
    }
}

#[test]
fn generate_length_replayed() {
    for case in cases()["generate_length"].as_array().unwrap() {
        let length = value(&case["length"]);
        let expected = case["result"].as_u64().unwrap() as usize;
        let numbers = Rc::new(ScriptedNumbers {
            answers: RefCell::new(vec![0; expected]),
            seen: RefCell::new(vec![]),
        });
        let (generator, filters) = generator(numbers);
        let password = generator
            .generate(&length, &Value::Bool(false), &Value::Bool(true))
            .unwrap();
        assert_eq!(
            password,
            Value::Str("a".repeat(expected)),
            "{}",
            case["name"]
        );
        let args = filters.random_args.borrow().clone().unwrap();
        assert_eq!(
            (args.1, args.2, args.3),
            (length, Value::Bool(false), Value::Bool(true)),
            "{}",
            case["name"]
        );
    }
}

#[test]
fn generate_out_of_range_index_adds_nothing() {
    let numbers = Rc::new(ScriptedNumbers {
        answers: RefCell::new(vec![1000, -1]),
        seen: RefCell::new(vec![]),
    });
    let (generator, _) = generator(numbers);
    // substr($chars, 1000, 1) is "", substr($chars, -1, 1) the last character.
    let password = generator
        .generate(&Value::Int(2), &Value::Bool(false), &Value::Bool(false))
        .unwrap();
    assert_eq!(password, Value::Str("9".into()));
}
