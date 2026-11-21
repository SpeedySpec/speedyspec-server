//! Reading recorded arguments the way PHP's own conversions would.

#![allow(dead_code)]

use speedyspec_wp_text_corpus::{Php, PhpKey};

/// PHP's `(int)` cast of a recorded scalar.
pub fn php_int(value: &Php) -> i64 {
    match value {
        Php::Null | Php::Bool(false) => 0,
        Php::Bool(true) => 1,
        Php::Int(i) => *i,
        Php::Float(f) => *f as i64,
        Php::Str(s) => {
            let text = String::from_utf8_lossy(s);
            let text = text.trim_start_matches([' ', '\t', '\n', '\r', '\x0B', '\x0C']);
            let end = text
                .char_indices()
                .find(|&(i, c)| !(c.is_ascii_digit() || (i == 0 && (c == '-' || c == '+'))))
                .map_or(text.len(), |(i, _)| i);
            text[..end].parse().unwrap_or(0)
        }
        other => panic!("not a scalar: {other}"),
    }
}

/// PHP's `empty()` on a recorded value.
pub fn php_empty(value: &Php) -> bool {
    match value {
        Php::Null | Php::Bool(false) => true,
        Php::Int(i) => *i == 0,
        Php::Float(f) => *f == 0.0,
        Php::Str(s) => s.is_empty() || s == b"0",
        Php::Array(a) => a.is_empty(),
        _ => false,
    }
}

/// A recorded string argument.
pub fn text(value: &Php) -> &[u8] {
    value.bytes().expect("a string argument")
}

/// A list of recorded strings, in the corpus's diagnostics shape.
pub fn report(items: &[&str], level: Option<i64>) -> Php {
    let mut list: Vec<Php> = items.iter().map(|s| Php::Str(s.as_bytes().to_vec())).collect();
    if let Some(level) = level {
        list.push(Php::Int(level));
    }
    Php::Array(vec![(
        PhpKey::Int(0),
        Php::Array(list.into_iter().enumerate().map(|(i, v)| (PhpKey::Int(i as i64), v)).collect()),
    )])
}

/// No diagnostics.
pub fn none() -> Php {
    Php::Array(Vec::new())
}
