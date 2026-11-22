//! PHP's `parse_str()`, which `wp_parse_str()` wraps.
//!
//! A transcription of PHP 8.5's `php_default_treat_data()` (`PARSE_STRING`) and `php_register_variable_ex()`
//! (`main/php_variables.c`), including what they do because they work on NUL-terminated C strings.

use speedyspec_hooks::{Key, Value};

use crate::strings::{bytes_value, urldecode};

/// PHP's `max_input_vars` default.
const MAX_INPUT_VARS: usize = 1000;
/// PHP's `max_input_nesting_level` default.
const MAX_INPUT_NESTING_LEVEL: usize = 64;

/// `parse_str( $input, $result )`'s `$result`.
///
/// @param input
///     The query string (`&`-separated; reading stops at the first NUL, as PHP's does).
/// @return
///     An array, nested where keys use `[...]`.
pub fn parse_str(input: &[u8]) -> Value {
    let input = until_nul(input);
    let mut top: Vec<(Key, Value)> = Vec::new();
    for (count, token) in input.split(|&b| b == b'&').filter(|t| !t.is_empty()).enumerate() {
        if count >= MAX_INPUT_VARS {
            break;
        }
        let (var, value) = match memchr::memchr(b'=', token) {
            Some(eq) => (&token[..eq], urldecode(&token[eq + 1..])),
            None => (token, Vec::new()),
        };
        let var = urldecode(var);
        register(until_nul(&var), value, &mut top);
    }
    Value::Array(top)
}

/// The bytes before the first NUL.
fn until_nul(bytes: &[u8]) -> &[u8] {
    memchr::memchr(0, bytes).map_or(bytes, |i| &bytes[..i])
}

/// C's `isspace()` in the C locale.
fn is_c_space(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | b'\x0B' | b'\x0C' | b'\r')
}

/// `strlen()` from `at` in a NUL-terminated buffer.
fn strlen(buf: &[u8], at: usize) -> usize {
    memchr::memchr(0, &buf[at..]).unwrap_or(buf.len() - at)
}

/// What a variable name resolves to.
enum Target {
    /// Walk these keys (`None` appends), creating arrays, then store at the last key.
    Store { walk: Vec<Option<Vec<u8>>>, last: Option<Vec<u8>> },
    /// Nested past `max_input_nesting_level`: the whole top-level variable is dropped.
    TooDeep(Vec<u8>),
}

/// `php_register_variable_ex()`.
fn register(var_name: &[u8], value: Vec<u8>, top: &mut Vec<(Key, Value)>) {
    let start = var_name.iter().take_while(|&&b| b == b' ').count();
    let pre_mangled = &var_name[start..];
    let mut buf = pre_mangled.to_vec();
    buf.push(0);
    let mut bracket = None;
    for i in 0..buf.len() - 1 {
        match buf[i] {
            b' ' | b'.' => buf[i] = b'_',
            b'[' => {
                bracket = Some(i);
                break;
            }
            _ => {}
        }
    }
    let var_len = bracket.unwrap_or(buf.len() - 1);
    if var_len == 0 {
        return;
    }
    let target = match bracket {
        None => Target::Store { walk: Vec::new(), last: Some(buf[..var_len].to_vec()) },
        Some(ip) => resolve(buf, ip, var_len),
    };
    match target {
        Target::TooDeep(var) => {
            let key = symtable_key(&var);
            top.retain(|(k, _)| *k != key);
        }
        Target::Store { walk, last } => store(top, &walk, last, value, pre_mangled),
    }
}

/// The nested-index loop of `php_register_variable_ex()`, on its mutable C-string buffer.
fn resolve(mut buf: Vec<u8>, mut ip: usize, var_len: usize) -> Target {
    buf[ip] = 0;
    let mut index: Option<usize> = Some(0);
    let mut walk = Vec::new();
    let mut nest = 0;
    let read = |buf: &[u8], at: Option<usize>| at.map(|at| buf[at..at + strlen(buf, at)].to_vec());
    loop {
        nest += 1;
        if nest > MAX_INPUT_NESTING_LEVEL {
            return Target::TooDeep(buf[..var_len].to_vec());
        }
        ip += 1;
        let index_s = ip;
        if is_c_space(buf[ip]) {
            ip += 1;
        }
        let next_index = if buf[ip] == b']' {
            None
        } else {
            match buf[ip..].iter().position(|&b| b == b']' || b == 0).filter(|&i| buf[ip + i] == b']') {
                None => {
                    buf[index_s - 1] = b'_';
                    let mut p = index_s;
                    while buf[p] != 0 {
                        if matches!(buf[p], b' ' | b'.' | b'[') {
                            buf[p] = b'_';
                        }
                        p += 1;
                    }
                    return Target::Store { walk, last: read(&buf, index) };
                }
                Some(i) => {
                    ip += i;
                    buf[ip] = 0;
                    Some(index_s)
                }
            }
        };
        walk.push(read(&buf, index));
        index = next_index;
        ip += 1;
        if buf[ip] == b'[' {
            buf[ip] = 0;
        } else {
            return Target::Store { walk, last: read(&buf, index) };
        }
    }
}

/// `php_is_forbidden_variable_name()`: a name that became `__Host-` or `__Secure-` only through mangling.
fn forbidden(mangled: &[u8], pre_mangled: &[u8]) -> bool {
    [&b"__Host-"[..], b"__Secure-"].iter().any(|p| mangled.starts_with(p) && !pre_mangled.starts_with(p))
}

/// `ZEND_HANDLE_NUMERIC_STR()`: a canonical decimal integer becomes an int key.
fn symtable_key(key: &[u8]) -> Key {
    let digits = key.strip_prefix(b"-").unwrap_or(key);
    let canonical = !digits.is_empty()
        && digits.iter().all(u8::is_ascii_digit)
        && (digits == b"0" || digits[0] != b'0')
        && key != b"-0";
    if canonical && let Ok(n) = std::str::from_utf8(key).expect("ASCII digits").parse::<i64>() {
        return Key::Int(n);
    }
    // The hook value type has string keys only; a key that isn't UTF-8 is read lossily (ledgered).
    Key::Str(String::from_utf8_lossy(key).into_owned())
}

/// The key `$array[] = ...` uses: one past the largest int key, or 0; `None` past `PHP_INT_MAX`, where PHP's
/// `zend_hash_next_index_insert()` fails and the value is dropped.
fn next_key(table: &[(Key, Value)]) -> Option<Key> {
    let max = table.iter().filter_map(|(k, _)| if let Key::Int(n) = k { Some(*n) } else { None }).max();
    match max {
        None => Some(Key::Int(0)),
        Some(n) => n.checked_add(1).map(Key::Int),
    }
}

/// Walks `walk`, creating or replacing arrays as PHP does, then stores the value.
fn store(top: &mut Vec<(Key, Value)>, walk: &[Option<Vec<u8>>], last: Option<Vec<u8>>, value: Vec<u8>, pre: &[u8]) {
    let mut table = top;
    for step in walk {
        let at = match step {
            None => {
                let Some(key) = next_key(table) else {
                    return;
                };
                table.push((key, Value::Array(Vec::new())));
                table.len() - 1
            }
            Some(name) => {
                if forbidden(name, pre) {
                    return;
                }
                let key = symtable_key(name);
                match table.iter().position(|(k, _)| *k == key) {
                    Some(i) => {
                        if !matches!(table[i].1, Value::Array(_)) {
                            table[i].1 = Value::Array(Vec::new());
                        }
                        i
                    }
                    None => {
                        table.push((key, Value::Array(Vec::new())));
                        table.len() - 1
                    }
                }
            }
        };
        let Value::Array(inner) = &mut table[at].1 else { unreachable!("made an array above") };
        table = inner;
    }
    let value = bytes_value(value);
    match last {
        None => {
            if let Some(key) = next_key(table) {
                table.push((key, value));
            }
        }
        Some(name) => {
            if forbidden(&name, pre) {
                return;
            }
            let key = symtable_key(&name);
            match table.iter_mut().find(|(k, _)| *k == key) {
                Some(slot) => slot.1 = value,
                None => table.push((key, value)),
            }
        }
    }
}
