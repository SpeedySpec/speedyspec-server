//! PHP's semantics where WordPress's rules rely on them: `empty()`, `isset()`, `(int)`, `reset()`, array indexing and
//! assignment that keeps a key's place.

use speedyspec_wp_media_contracts::Tree;

/// PHP's `empty()`.
pub fn empty(value: Option<&Tree>) -> bool {
    match value {
        None | Some(Tree::Null) | Some(Tree::Bool(false)) => true,
        Some(Tree::Int(0)) => true,
        Some(Tree::Float(f)) => *f == 0.0,
        Some(Tree::Bytes(b)) => b.is_empty() || b == b"0",
        Some(Tree::Str(s)) => s.is_empty() || s == "0",
        Some(Tree::List(l)) => l.is_empty(),
        Some(Tree::Map(m)) => m.is_empty(),
        _ => false,
    }
}

/// PHP's `isset()` on a value already looked up.
pub fn isset(value: Option<&Tree>) -> bool {
    !matches!(value, None | Some(Tree::Null))
}

/// `$array[$key]`, for string keys and for integer keys of lists or of maps keyed `"0"`, `"1"`….
pub fn get<'a>(value: Option<&'a Tree>, key: &str) -> Option<&'a Tree> {
    match value? {
        Tree::Map(m) => m.iter().find(|(k, _)| k == key).map(|(_, v)| v),
        Tree::List(l) => key.parse::<usize>().ok().and_then(|i| l.get(i)),
        _ => None,
    }
}

/// `$array[$a][$b]…`.
pub fn path<'a>(value: &'a Tree, keys: &[&str]) -> Option<&'a Tree> {
    keys.iter().try_fold(value, |t, k| get(Some(t), k))
}

/// PHP's `reset()`: the first element, whatever its key.
pub fn reset(value: &Tree) -> Option<&Tree> {
    match value {
        Tree::Map(m) => m.first().map(|(_, v)| v),
        Tree::List(l) => l.first(),
        _ => None,
    }
}

/// The entries of an array, keys as PHP would give them.
pub fn entries(value: &Tree) -> Vec<(String, &Tree)> {
    match value {
        Tree::Map(m) => m.iter().map(|(k, v)| (k.clone(), v)).collect(),
        Tree::List(l) => l
            .iter()
            .enumerate()
            .map(|(i, v)| (i.to_string(), v))
            .collect(),
        _ => Vec::new(),
    }
}

/// PHP's `(int)` cast.
pub fn int(value: &Tree) -> i64 {
    match value {
        Tree::Int(i) => *i,
        Tree::Float(f) if f.is_finite() => f.trunc() as i64,
        Tree::Bool(b) => i64::from(*b),
        Tree::Bytes(b) => leading_int(b),
        Tree::Str(s) => leading_int(s.as_bytes()),
        Tree::List(l) => i64::from(!l.is_empty()),
        Tree::Map(m) => i64::from(!m.is_empty()),
        _ => 0,
    }
}

/// The integer a numeric string starts with, as PHP's `(int)` reads it (leading whitespace, sign, digits).
fn leading_int(bytes: &[u8]) -> i64 {
    let text = String::from_utf8_lossy(bytes);
    let t = text.trim_start();
    let (sign, digits) = match t.strip_prefix('-') {
        Some(rest) => (-1, rest),
        None => (1, t.strip_prefix('+').unwrap_or(t)),
    };
    let n: String = digits.chars().take_while(char::is_ascii_digit).collect();
    if n.is_empty() {
        return 0;
    }
    // A float-looking string ("12.9e2") casts through its float value in PHP 8.
    if let Ok(f) = t.parse::<f64>() {
        return f.trunc() as i64;
    }
    n.parse::<i64>()
        .map(|v| sign * v)
        .unwrap_or(if sign < 0 { i64::MIN } else { i64::MAX })
}

/// `$array[$key] = $value`, keeping the key's place when it exists, as PHP does.
pub fn set(array: &mut Tree, key: &str, value: Tree) {
    if !matches!(array, Tree::Map(_)) {
        *array = Tree::Map(Vec::new());
    }
    if let Tree::Map(m) = array {
        match m.iter_mut().find(|(k, _)| k == key) {
            Some(entry) => entry.1 = value,
            None => m.push((key.to_owned(), value)),
        }
    }
}

/// `unset($array[$key])`.
pub fn unset(array: &mut Tree, key: &str) {
    if let Tree::Map(m) = array {
        m.retain(|(k, _)| k != key);
    }
}
