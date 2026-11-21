//! The native functions `utf8.php` calls. The facade has already coerced every argument to the type WordPress's
//! signature names, so each reads plain strings, integers and nulls.

use speedyspec_hooks::{Key, Value};
use speedyspec_php::ext_php_rs::builders::FunctionBuilder;
use speedyspec_php::native::{self, NativeError};
use speedyspec_wp_utf8 as utf8;

const NATIVE: &str = "SpeedySpec\\WpUtf8\\Native\\";

type R = Result<Value, NativeError>;

fn bytes(args: &[Value], i: usize) -> Result<&[u8], NativeError> {
    match args.get(i) {
        Some(Value::Str(s)) => Ok(s.as_bytes()),
        Some(Value::Bytes(b)) => Ok(b),
        other => Err(NativeError::throw(
            "TypeError",
            format!("argument #{} must be a string, {other:?} given", i + 1),
        )),
    }
}

fn int(args: &[Value], i: usize) -> Option<i64> {
    match args.get(i) {
        Some(Value::Int(n)) => Some(*n),
        _ => None,
    }
}

fn string(b: &[u8]) -> Value {
    match std::str::from_utf8(b) {
        Ok(s) => Value::Str(s.to_owned()),
        Err(_) => Value::Bytes(b.to_vec()),
    }
}

fn list(items: Vec<Value>) -> Value {
    Value::Array(
        items
            .into_iter()
            .enumerate()
            .map(|(i, v)| (Key::Int(i as i64), v))
            .collect(),
    )
}

fn usize_or_zero(n: i64) -> usize {
    usize::try_from(n).unwrap_or(0)
}

/// `[count, at, invalid_length, has_noncharacters, noncharacters_tested]`.
fn scan(args: &[Value]) -> R {
    let s = bytes(args, 0)?;
    let at = int(args, 1).unwrap_or(0);
    let max_bytes = int(args, 2);
    let max_cp = int(args, 3);
    // WordPress's loop never starts for a negative code point limit, nor past the end; `$at` comes back unchanged.
    if at < 0 || max_cp.is_some_and(|n| n < 0) {
        return Ok(list(vec![
            Value::Int(0),
            Value::Int(at),
            Value::Int(0),
            Value::Bool(false),
            Value::Bool(false),
        ]));
    }
    let r = utf8::scan_utf8(
        s,
        usize_or_zero(at),
        max_bytes.map(usize_or_zero),
        max_cp.map(usize_or_zero),
    );
    Ok(list(vec![
        Value::Int(r.code_points as i64),
        Value::Int(r.at.max(usize_or_zero(at)) as i64),
        Value::Int(r.invalid_length as i64),
        Value::Bool(r.has_noncharacters),
        Value::Bool(r.noncharacters_tested),
    ]))
}

/// `[bytes, found]`.
fn span(args: &[Value]) -> R {
    let s = bytes(args, 0)?;
    let offset = usize_or_zero(int(args, 1).unwrap_or(0));
    let max = usize_or_zero(int(args, 2).unwrap_or(0));
    let (spanned, found) = utf8::codepoint_span(s, offset, max);
    Ok(list(vec![
        Value::Int(spanned as i64),
        Value::Int(found as i64),
    ]))
}

pub(crate) fn functions() -> Vec<FunctionBuilder<'static>> {
    let f =
        |name: &str, body: fn(&[Value]) -> R| native::function(&format!("{NATIVE}{name}"), body);
    vec![
        f("is_valid_utf8", |a| {
            Ok(Value::Bool(utf8::is_valid_utf8(bytes(a, 0)?)))
        }),
        f("is_valid_utf8_fallback", |a| {
            Ok(Value::Bool(utf8::is_valid_utf8_reference(bytes(a, 0)?)))
        }),
        f("scrub_utf8", |a| {
            Ok(string(&utf8::scrub_utf8(bytes(a, 0)?)))
        }),
        f("has_noncharacters", |a| {
            Ok(Value::Bool(utf8::has_noncharacters(bytes(a, 0)?)))
        }),
        f("scan_utf8", scan),
        f("codepoint_count", |a| {
            let s = bytes(a, 0)?;
            Ok(Value::Int(utf8::codepoint_count(
                s,
                usize_or_zero(int(a, 1).unwrap_or(0)),
                usize_or_zero(int(a, 2).unwrap_or(0)),
            ) as i64))
        }),
        f("codepoint_span", span),
        f("utf8_encode", |a| {
            Ok(string(&utf8::utf8_encode_latin1(bytes(a, 0)?)))
        }),
        f("utf8_decode", |a| {
            Ok(string(&utf8::utf8_decode_latin1(bytes(a, 0)?)))
        }),
        f("is_utf8_charset", |a| {
            Ok(Value::Bool(utf8::is_utf8_charset(bytes(a, 0)?)))
        }),
        f("mb_substr", |a| {
            Ok(string(&utf8::mb_substr(
                bytes(a, 0)?,
                int(a, 1).unwrap_or(0),
                int(a, 2),
                true,
            )))
        }),
        f("mb_strlen", |a| {
            Ok(Value::Int(utf8::mb_strlen(bytes(a, 0)?, true) as i64))
        }),
    ]
}
