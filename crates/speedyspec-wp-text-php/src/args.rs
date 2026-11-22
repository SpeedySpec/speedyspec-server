//! Reading native arguments as WordPress's PHP bodies read them, and failing as PHP fails.

use speedyspec_php::native::NativeError;
use speedyspec_php_ext::Value;
use speedyspec_wp_kses_contracts::KsesError;
use speedyspec_wp_text::strings::to_php_string;
use speedyspec_wp_text_contracts::TextError;

/// Throws PHP's `ArgumentCountError` for a user function called with too few arguments.
///
/// @param name
///     The function's WordPress name.
/// @param args
///     What it was given.
/// @param required
///     How many parameters have no default.
/// @param max
///     How many parameters it has (for PHP's "exactly" or "at least").
/// @return
///     Nothing, or the error.
pub(crate) fn required(name: &str, args: &[Value], required: usize, max: usize) -> Result<(), NativeError> {
    if args.len() >= required {
        return Ok(());
    }
    let bound = if required == max { "exactly" } else { "at least" };
    Err(NativeError::throw(
        "ArgumentCountError",
        format!("Too few arguments to function {name}(), {} passed and {bound} {required} expected", args.len()),
    ))
}

/// Argument `i`, or `null` when missing.
pub(crate) fn arg(args: &[Value], i: usize) -> Value {
    args.get(i).map(|v| v.read().into_owned()).unwrap_or(Value::Null)
}

/// Argument `i` as PHP's `(string)`: an object or an array goes through PHP's own `strval()` (an object's
/// `__toString()`, or PHP's error for an object without one; `"Array"` and PHP's warning for an array).
pub(crate) fn string(args: &[Value], i: usize) -> Result<Vec<u8>, NativeError> {
    let value = arg(args, i);
    if let Value::Opaque(_) | Value::Array(_) = value {
        let cast = speedyspec_php::call_catching(&Value::Str("strval".into()), &[value])
            .map_err(|f| native_error(crate::adapters::call_error(f)))?;
        return Ok(to_php_string(&cast.read()).unwrap_or_default());
    }
    Ok(to_php_string(&value).unwrap_or_default())
}

/// Argument `i` as PHP's `(string)`, or `default` when it wasn't passed.
pub(crate) fn string_or(args: &[Value], i: usize, default: &[u8]) -> Result<Vec<u8>, NativeError> {
    if args.len() > i { string(args, i) } else { Ok(default.to_vec()) }
}

/// Argument `i`, with an object that has `__toString()` already cast, for the functions WordPress lets take one.
pub(crate) fn stringable(args: &[Value], i: usize) -> Result<Value, NativeError> {
    let value = arg(args, i);
    if let Value::Opaque(_) = value {
        let has = speedyspec_php::call_catching(&Value::Str("method_exists".into()), &[value.clone(), Value::Str("__toString".into())])
            .map_err(|f| native_error(crate::adapters::call_error(f)))?;
        if matches!(has, Value::Bool(true)) {
            return Ok(speedyspec_wp_text::strings::bytes_value(string(args, i)?));
        }
    }
    Ok(value)
}

/// Argument `i` as PHP's truthiness, or `default` when it wasn't passed.
pub(crate) fn bool_or(args: &[Value], i: usize, default: bool) -> bool {
    if args.len() <= i {
        return default;
    }
    match arg(args, i) {
        Value::Null | Value::Bool(false) => false,
        Value::Int(n) => n != 0,
        Value::Float(f) => f != 0.0,
        Value::Str(s) => !(s.is_empty() || s == "0"),
        Value::Bytes(b) => !(b.is_empty() || b == b"0"),
        Value::Array(a) => !a.is_empty(),
        _ => true,
    }
}

/// Argument `i` as PHP's `(int)`, or `default` when it wasn't passed.
pub(crate) fn int_or(args: &[Value], i: usize, default: i64) -> i64 {
    if args.len() <= i {
        return default;
    }
    match arg(args, i) {
        Value::Null | Value::Bool(false) => 0,
        Value::Bool(true) => 1,
        Value::Int(n) => n,
        Value::Float(f) => f as i64,
        Value::Str(_) | Value::Bytes(_) => {
            let text = to_php_string(&arg(args, i)).unwrap_or_default();
            let text = String::from_utf8_lossy(&text);
            let trimmed = text.trim_start_matches([' ', '\t', '\n', '\r', '\x0B', '\x0C']);
            let end = trimmed
                .char_indices()
                .find(|&(j, c)| !(c.is_ascii_digit() || (j == 0 && (c == '-' || c == '+'))))
                .map_or(trimmed.len(), |(j, _)| j);
            trimmed[..end].parse().unwrap_or(0)
        }
        _ => 1,
    }
}

/// A text error as PHP sees it: a PHP exception rethrown as the same object, PHP unwinding left alone, anything else
/// an `Error`.
///
/// @param e
///     The error, whose payload may be a PHP exception `Value`, a kses error carrying one, or an unwinding mark.
/// @return
///     The native error.
pub(crate) fn native_error(e: TextError) -> NativeError {
    let Some(payload) = e.payload else {
        return NativeError::throw("Error", e.message);
    };
    if let Some(exception) = payload.downcast_ref::<Value>() {
        return NativeError::Rethrow(exception.clone());
    }
    if let Some(KsesError::Foreign { payload, .. }) = payload.downcast_ref::<KsesError>() {
        return match payload.0.downcast_ref::<Value>() {
            Some(exception) => NativeError::Rethrow(exception.clone()),
            None => NativeError::Silent,
        };
    }
    if payload.downcast_ref::<KsesError>().is_some() {
        return NativeError::throw("Error", e.message);
    }
    // Any other payload is a bridge's mark that PHP is unwinding (a fatal error, `exit()`).
    NativeError::Silent
}
