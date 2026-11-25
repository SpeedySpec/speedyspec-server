//! The formatting corpus, read with no PHP.
//!
//! `compat/formatting` holds one-call scenarios built from WordPress's own PHPUnit data providers, and what stock
//! WordPress returned for each (recorded by `speedyspec-compat`). Each text crate checks its functions against it
//! byte for byte with [`assert_corpus`]; [`coverage`] says how much of the corpus the ported functions answer.

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};

use base64::Engine;
use serde::Deserialize;
use serde_json::Value;

mod bench;
#[cfg(feature = "values")]
mod values;

pub use bench::{inputs, page, short_inputs};
#[cfg(feature = "values")]
pub use values::{diagnostics, from_value, to_value};

/// A value as WordPress recorded it.
#[derive(Clone, Debug, PartialEq)]
pub enum Php {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    /// A PHP string: bytes, which need not be UTF-8.
    Str(Vec<u8>),
    /// An ordered PHP array.
    Array(Vec<(PhpKey, Php)>),
    /// The call threw.
    Threw { class: String, message: String },
    /// Anything else the recorder can report (an object, a callback), kept as recorded.
    Other(Value),
}

/// A PHP array key.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PhpKey {
    Int(i64),
    Str(Vec<u8>),
}

impl Php {
    /// The bytes of a string value.
    pub fn bytes(&self) -> Option<&[u8]> {
        match self {
            Php::Str(bytes) => Some(bytes),
            _ => None,
        }
    }

    /// A list (`0..n` keys) of string values.
    pub fn list<'a>(items: impl IntoIterator<Item = &'a [u8]>) -> Php {
        Php::Array(
            items
                .into_iter()
                .enumerate()
                .map(|(i, item)| (PhpKey::Int(i as i64), Php::Str(item.to_vec())))
                .collect(),
        )
    }
}

impl fmt::Display for Php {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Php::Null => f.write_str("null"),
            Php::Bool(b) => write!(f, "{b}"),
            Php::Int(i) => write!(f, "{i}"),
            Php::Float(x) => write!(f, "{x:?}"),
            Php::Str(bytes) => write!(f, "\"{}\"", bytes.escape_ascii()),
            Php::Array(pairs) => {
                f.write_str("[")?;
                for (i, (key, value)) in pairs.iter().enumerate() {
                    if i > 0 {
                        f.write_str(", ")?;
                    }
                    match key {
                        PhpKey::Int(k) => write!(f, "{k} => {value}")?,
                        PhpKey::Str(k) => write!(f, "\"{}\" => {value}", k.escape_ascii())?,
                    }
                }
                f.write_str("]")
            }
            Php::Threw { class, message } => write!(f, "threw {class}: {message}"),
            Php::Other(value) => write!(f, "{value}"),
        }
    }
}

/// Reads a value in the recorder's encoding.
pub fn decode(value: &Value) -> Php {
    match value {
        Value::Null => Php::Null,
        Value::Bool(b) => Php::Bool(*b),
        Value::Number(n) => match n.as_i64() {
            Some(i) => Php::Int(i),
            None => Php::Float(n.as_f64().unwrap_or(f64::NAN)),
        },
        Value::String(s) => Php::Str(s.as_bytes().to_vec()),
        Value::Object(map) if map.len() == 1 && map.contains_key("bytes") => {
            let encoded = map["bytes"].as_str().unwrap_or_default();
            Php::Str(
                base64::engine::general_purpose::STANDARD
                    .decode(encoded)
                    .unwrap_or_else(|e| panic!("bad base64 {encoded:?} in the corpus: {e}")),
            )
        }
        Value::Object(map) if map.len() == 1 && map.contains_key("float") => match &map["float"] {
            Value::Number(n) => Php::Float(n.as_f64().unwrap_or(f64::NAN)),
            Value::String(s) => Php::Float(match s.as_str() {
                "INF" => f64::INFINITY,
                "-INF" => f64::NEG_INFINITY,
                _ => f64::NAN,
            }),
            other => Php::Other(other.clone()),
        },
        Value::Object(map) if map.len() == 1 && map.contains_key("array") => {
            let pairs = map["array"].as_array().cloned().unwrap_or_default();
            Php::Array(
                pairs
                    .iter()
                    .map(|pair| {
                        let key = match &pair[0] {
                            Value::Number(n) => PhpKey::Int(n.as_i64().unwrap_or_default()),
                            Value::String(s) => PhpKey::Str(s.as_bytes().to_vec()),
                            other => match decode(other) {
                                Php::Str(bytes) => PhpKey::Str(bytes),
                                _ => panic!("an array key the corpus cannot hold: {other}"),
                            },
                        };
                        (key, decode(&pair[1]))
                    })
                    .collect(),
            )
        }
        Value::Object(map) if map.contains_key("threw") => Php::Threw {
            class: map["threw"].as_str().unwrap_or_default().to_string(),
            message: map
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
        },
        other => Php::Other(other.clone()),
    }
}

/// One recorded call.
#[derive(Clone, Debug, PartialEq)]
pub struct Case {
    pub scenario: String,
    pub function: String,
    pub args: Vec<Php>,
    pub returned: Php,
    /// What WordPress reported while running it: deprecations and triggered errors.
    pub diagnostics: Php,
}

#[derive(Deserialize)]
struct Catalogue {
    scenarios: Vec<Scenario>,
}

#[derive(Deserialize)]
struct Scenario {
    name: String,
    steps: Vec<Step>,
}

#[derive(Deserialize)]
struct Step {
    call: String,
    #[serde(default)]
    args: Vec<Value>,
}

#[derive(Deserialize)]
struct Fixture {
    results: BTreeMap<String, Vec<StepResult>>,
}

#[derive(Deserialize)]
struct StepResult {
    returned: Value,
    #[serde(default)]
    state: Value,
}

#[derive(Deserialize)]
struct Breaks {
    breaks: Vec<Break>,
}

#[derive(Deserialize)]
struct Break {
    scenario: String,
    #[serde(default)]
    step: Option<usize>,
}

#[derive(Deserialize)]
struct Source {
    wordpress: String,
}

/// The WordPress release the corpus was recorded from: `compat/formatting/source.json`'s `wordpress`.
pub fn wordpress() -> String {
    read::<Source>(&compat_dir().join("source.json")).wordpress
}

fn compat_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../compat/formatting")
}

fn read<T: for<'de> Deserialize<'de>>(path: &Path) -> T {
    let text = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("the corpus file {} cannot be read: {e}", path.display()));
    serde_json::from_str(&text)
        .unwrap_or_else(|e| panic!("the corpus file {} is not valid: {e}", path.display()))
}

fn all_cases() -> Vec<Case> {
    let dir = compat_dir();
    let catalogue: Catalogue = read(&dir.join("scenarios.json"));
    let fixture: Fixture = read(&dir.join(format!("wordpress-{}.json", wordpress())));
    let breaks: Breaks = read(&dir.join("breaks.json"));
    catalogue
        .scenarios
        .into_iter()
        .filter(|s| s.steps.len() == 1)
        .filter(|s| {
            !breaks
                .breaks
                .iter()
                .any(|b| b.scenario == s.name && b.step.is_none_or(|step| step == 0))
        })
        .map(|s| {
            let result = fixture
                .results
                .get(&s.name)
                .and_then(|steps| steps.first())
                .unwrap_or_else(|| {
                    panic!("the corpus fixture has no result for {}", s.name)
                });
            let step = &s.steps[0];
            Case {
                scenario: s.name.clone(),
                function: step.call.clone(),
                args: step.args.iter().map(decode).collect(),
                returned: decode(&result.returned),
                diagnostics: decode(&result.state),
            }
        })
        .collect()
}

/// Every recorded call of `function`, leaving out documented breaks.
pub fn cases(function: &str) -> Vec<Case> {
    all_cases()
        .into_iter()
        .filter(|c| c.function == function)
        .collect()
}

/// Checks `run` against every recorded call of `function`, and panics listing every case it gets wrong.
///
/// It is for functions that report nothing: a case where WordPress reported a deprecation or an error is a mismatch
/// here, so such a function must use [`assert_corpus_reporting`]. It also panics when `function` has no cases, so a
/// misspelt name cannot pass.
pub fn assert_corpus(function: &str, run: impl Fn(&[Php]) -> Php) {
    assert_corpus_reporting(function, |args| (run(args), Php::Array(Vec::new())));
}

/// Checks `run` against every recorded call of `function`, comparing what it returns and what it reported (the
/// recorder's diagnostics: a list of `[kind, function, ...arguments]`), and panics listing every case it gets wrong.
pub fn assert_corpus_reporting(function: &str, run: impl Fn(&[Php]) -> (Php, Php)) {
    let cases = cases(function);
    assert!(
        !cases.is_empty(),
        "the corpus has no cases for {function}"
    );
    let wrong = mismatches(&cases, run);
    assert!(
        wrong.is_empty(),
        "{} of {} {function} cases differ from WordPress:\n{}",
        wrong.len(),
        cases.len(),
        wrong.join("\n")
    );
}

/// Every case `run` gets wrong, described: its result or its diagnostics differ from what WordPress recorded.
pub fn mismatches(cases: &[Case], run: impl Fn(&[Php]) -> (Php, Php)) -> Vec<String> {
    cases
        .iter()
        .filter_map(|case| {
            let (actual, reported) = run(&case.args);
            if actual == case.returned && reported == case.diagnostics {
                return None;
            }
            let args: Vec<String> = case.args.iter().map(ToString::to_string).collect();
            Some(format!(
                "{}\n  args:        ({})\n  expected:    {}\n  actual:      {actual}\n  diagnostics: expected {}, actual {reported}",
                case.scenario,
                args.join(", "),
                case.returned,
                case.diagnostics
            ))
        })
        .collect()
}

/// How much of the corpus the ported functions answer.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Coverage {
    pub covered: usize,
    /// The cases per function that no crate implements yet: skipped, neither passed nor failed.
    pub skipped: BTreeMap<String, usize>,
}

/// Counts the cases whose function is in `implemented`, and the skipped cases of every other function.
pub fn coverage(implemented: &[&str]) -> Coverage {
    let mut coverage = Coverage::default();
    for case in all_cases() {
        if implemented.contains(&case.function.as_str()) {
            coverage.covered += 1;
        } else {
            *coverage.skipped.entry(case.function).or_default() += 1;
        }
    }
    coverage
}
