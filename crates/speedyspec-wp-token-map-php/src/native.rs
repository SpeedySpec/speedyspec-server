//! The native functions the facade's `WP_Token_Map` calls (`SpeedySpec\WpTokenMap\Native\`).

use std::cell::Cell;

use speedyspec_hooks::{Key, Value};
use speedyspec_php::ext_php_rs::builders::FunctionBuilder;
use speedyspec_php::native::{self, NativeError};
use speedyspec_wp_token_map::{CaseSensitivity, STORAGE_VERSION, html5};

const NATIVE: &str = "SpeedySpec\\WpTokenMap\\Native\\";

type R = Result<Value, NativeError>;

thread_local! {
    /// Lookups answered natively (for tests).
    static LOOKUPS: Cell<u64> = const { Cell::new(0) };
    /// The longest text `html5_read_token()` was handed (for tests: the facade passes a window, not the document).
    static LONGEST: Cell<u64> = const { Cell::new(0) };
}

fn bytes(v: Option<&Value>) -> Option<Vec<u8>> {
    match v?.read().as_ref() {
        Value::Str(s) => Some(s.as_bytes().to_vec()),
        Value::Bytes(b) => Some(b.clone()),
        _ => None,
    }
}

fn case(v: Option<&Value>) -> CaseSensitivity {
    bytes(v).map_or(CaseSensitivity::CaseSensitive, |c| {
        CaseSensitivity::from_php(&c)
    })
}

fn counted() {
    LOOKUPS.with(|c| c.set(c.get() + 1));
}

/// A list of strings (keys 0, 1, 2, … in order, as WordPress indexes `large_words` and `small_mappings`).
fn strings(v: Option<&Value>) -> Option<Vec<Vec<u8>>> {
    match v?.read().as_ref() {
        Value::Array(items) => items
            .iter()
            .enumerate()
            .map(|(i, (k, v))| {
                if *k == Key::Int(i as i64) {
                    bytes(Some(v))
                } else {
                    None
                }
            })
            .collect(),
        _ => None,
    }
}

fn field<'a>(state: &'a [(Key, Value)], name: &str) -> Option<&'a Value> {
    state
        .iter()
        .find(|(k, _)| *k == Key::Str(name.to_owned()))
        .map(|(_, v)| v)
}

/// Whether a precomputed table's `$state` is exactly WordPress's HTML5 table.
fn is_html5_table(a: &[Value]) -> R {
    let Some(Value::Array(state)) = a.first().map(|v| v.read().into_owned()) else {
        return Ok(Value::Bool(false));
    };
    let table = html5();
    let same_list = |name: &str, want: &[&[u8]]| {
        strings(field(&state, name))
            .is_some_and(|got| got.len() == want.len() && got.iter().zip(want).all(|(g, w)| g == w))
    };
    let is = bytes(field(&state, "storage_version")).as_deref() == Some(STORAGE_VERSION.as_bytes())
        && matches!(field(&state, "key_length").map(|v| v.read().into_owned()), Some(Value::Int(n)) if n as usize == table.key_length)
        && bytes(field(&state, "groups")).as_deref() == Some(table.groups)
        && bytes(field(&state, "small_words")).as_deref() == Some(table.small_words)
        && same_list("large_words", &table.large_words.slices())
        && same_list("small_mappings", &table.small_mappings.slices());
    Ok(Value::Bool(is))
}

fn bytes_value(b: &[u8]) -> Value {
    match std::str::from_utf8(b) {
        Ok(s) => Value::Str(s.to_owned()),
        Err(_) => Value::Bytes(b.to_vec()),
    }
}

/// `read_token()` on the HTML5 table: `[replacement, length]` or null.
fn html5_read_token(a: &[Value]) -> R {
    let text =
        bytes(a.first()).ok_or_else(|| NativeError::throw("TypeError", "text must be a string"))?;
    let offset = match a.get(1).map(|v| v.read().into_owned()) {
        Some(Value::Int(n)) if n >= 0 => n as usize,
        _ => {
            return Err(NativeError::throw(
                "TypeError",
                "offset must be a non-negative int",
            ));
        }
    };
    counted();
    LONGEST.with(|l| l.set(l.get().max(text.len() as u64)));
    Ok(match html5().read_token(&text, offset, case(a.get(2))) {
        Some((mapping, length)) => Value::Array(vec![
            (Key::Int(0), bytes_value(mapping)),
            (Key::Int(1), Value::Int(length as i64)),
        ]),
        None => Value::Null,
    })
}

/// `contains()` on the HTML5 table.
fn html5_contains(a: &[Value]) -> R {
    let word =
        bytes(a.first()).ok_or_else(|| NativeError::throw("TypeError", "word must be a string"))?;
    counted();
    Ok(Value::Bool(html5().contains(&word, case(a.get(1)))))
}

pub(crate) fn functions() -> Vec<FunctionBuilder<'static>> {
    let f =
        |name: &str, body: fn(&[Value]) -> R| native::function(&format!("{NATIVE}{name}"), body);
    vec![
        f("is_html5_table", is_html5_table),
        f("html5_read_token", html5_read_token),
        f("html5_contains", html5_contains),
        f("stats", |_| Ok(Value::Int(LOOKUPS.with(Cell::get) as i64))),
        f("longest_text", |_| {
            Ok(Value::Int(LONGEST.with(Cell::get) as i64))
        }),
    ]
}
