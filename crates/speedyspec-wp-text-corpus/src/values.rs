//! Recorded values as the Hook API's values, and back, for functions that take or return PHP values.

use speedyspec_hooks::{Key, Value};
use speedyspec_wp_text_memory::Diagnostic;
use speedyspec_wp_text_contracts::ErrorLevel;

use crate::{Php, PhpKey};

/// A recorded value as the Hook API carries it.
///
/// A string that is valid UTF-8 becomes `Value::Str`, any other `Value::Bytes`, as PHP strings reach Rust.
/// Panics on a thrown call or an unrecognised value, which no test passes as an argument.
pub fn to_value(php: &Php) -> Value {
    match php {
        Php::Null => Value::Null,
        Php::Bool(b) => Value::Bool(*b),
        Php::Int(i) => Value::Int(*i),
        Php::Float(f) => Value::Float(*f),
        Php::Str(bytes) => match String::from_utf8(bytes.clone()) {
            Ok(s) => Value::Str(s),
            Err(e) => Value::Bytes(e.into_bytes()),
        },
        Php::Array(pairs) => Value::Array(
            pairs
                .iter()
                .map(|(k, v)| {
                    let key = match k {
                        PhpKey::Int(i) => Key::Int(*i),
                        PhpKey::Str(s) => Key::Str(String::from_utf8_lossy(s).into_owned()),
                    };
                    (key, to_value(v))
                })
                .collect(),
        ),
        other => panic!("not a value a test passes: {other}"),
    }
}

/// A Hook API value as the recorder would have written it, for comparing with the corpus.
///
/// Panics on an opaque value, which the corpus cannot hold.
pub fn from_value(value: &Value) -> Php {
    match value {
        Value::Null => Php::Null,
        Value::Bool(b) => Php::Bool(*b),
        Value::Int(i) => Php::Int(*i),
        Value::Float(f) => Php::Float(*f),
        Value::Str(s) => Php::Str(s.as_bytes().to_vec()),
        Value::Bytes(b) => Php::Str(b.clone()),
        Value::Array(pairs) => Php::Array(
            pairs
                .iter()
                .map(|(k, v)| {
                    let key = match k {
                        Key::Int(i) => PhpKey::Int(*i),
                        Key::Str(s) => PhpKey::Str(s.as_bytes().to_vec()),
                    };
                    (key, from_value(v))
                })
                .collect(),
        ),
        Value::Opaque(_) => panic!("an opaque value"),
    }
}

/// What a `RecordingDiagnostics` kept, in the recorder's shape: a list of `[kind, function, ...arguments]`.
///
/// Error levels are PHP's `E_USER_*` integers, as `wp_trigger_error()` receives them.
pub fn diagnostics(list: &[Diagnostic]) -> Php {
    let s = |text: &str| Php::Str(text.as_bytes().to_vec());
    Php::Array(
        list.iter()
            .enumerate()
            .map(|(i, d)| {
                let items = match d {
                    Diagnostic::DeprecatedFunction { function, version, replacement } => {
                        vec![s("deprecated_function"), s(function), s(version), s(replacement)]
                    }
                    Diagnostic::DeprecatedArgument { function, version, message } => {
                        vec![s("deprecated_argument"), s(function), s(version), s(message)]
                    }
                    Diagnostic::TriggeredError { function, message, level } => {
                        let level = match level {
                            ErrorLevel::Error => 256,
                            ErrorLevel::Warning => 512,
                            ErrorLevel::Notice => 1024,
                            ErrorLevel::Deprecated => 16384,
                        };
                        vec![s("trigger_error"), s(function), s(message), Php::Int(level)]
                    }
                };
                (
                    PhpKey::Int(i as i64),
                    Php::Array(items.into_iter().enumerate().map(|(j, v)| (PhpKey::Int(j as i64), v)).collect()),
                )
            })
            .collect(),
    )
}
