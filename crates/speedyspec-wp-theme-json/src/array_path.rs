//! WordPress's array path helpers (`_wp_array_get`, `_wp_array_set`, `isset`) over PHP values.

use speedyspec_hooks::{Key, Value};

/// PHP's array key normalisation: a canonical decimal integer string is an integer key.
pub fn php_key(name: &str) -> Key {
    match name.parse::<i64>() {
        Ok(i) if i.to_string() == name => Key::Int(i),
        _ => Key::Str(name.to_owned()),
    }
}

/// `$array[$key]` when `$array` is an array that has the key.
pub fn child<'a>(value: &'a Value, key: &Key) -> Option<&'a Value> {
    match value {
        Value::Array(entries) => entries.iter().find(|(k, _)| k == key).map(|(_, v)| v),
        _ => None,
    }
}

fn child_mut<'a>(value: &'a mut Value, key: &Key) -> Option<&'a mut Value> {
    match value {
        Value::Array(entries) => entries.iter_mut().find(|(k, _)| k == key).map(|(_, v)| v),
        _ => None,
    }
}

/// `_wp_array_get( $array, $path )`: the value at `path` (which may be `null`), or `None` when a step is missing.
pub fn get<'a>(value: &'a Value, path: &[&str]) -> Option<&'a Value> {
    if path.is_empty() {
        return None;
    }
    path.iter()
        .try_fold(value, |current, name| child(current, &php_key(name)))
}

/// `isset( $array[a][b]… )`: the value exists and is not `null`.
pub fn isset(value: &Value, path: &[&str]) -> bool {
    get(value, path).is_some_and(|v| !matches!(v, Value::Null))
}

/// `_wp_array_set( $array, $path, $value )`: creates missing or non-array steps as empty arrays; does nothing when
/// `value` is not an array.
pub fn set(target: &mut Value, path: &[&str], new: Value) {
    if path.is_empty() || !matches!(target, Value::Array(_)) {
        return;
    }
    let mut current = target;
    for name in &path[..path.len() - 1] {
        let key = php_key(name);
        let is_array = matches!(child(current, &key), Some(Value::Array(_)));
        if !is_array {
            insert(current, key.clone(), Value::Array(Vec::new()));
        }
        current = child_mut(current, &key).expect("just inserted");
    }
    insert(current, php_key(path[path.len() - 1]), new);
}

/// `$array[$key] = $value`: replaces in place, or appends.
pub fn insert(target: &mut Value, key: Key, new: Value) {
    if let Value::Array(entries) = target {
        match entries.iter_mut().find(|(k, _)| *k == key) {
            Some(slot) => slot.1 = new,
            None => entries.push((key, new)),
        }
    }
}

/// `unset( $array[a][b]… )`, when every step before the last is an array.
pub fn unset(target: &mut Value, path: &[&str]) {
    let Some((last, init)) = path.split_last() else {
        return;
    };
    let mut current = target;
    for name in init {
        match child_mut(current, &php_key(name)) {
            Some(next) => current = next,
            None => return,
        }
    }
    if let Value::Array(entries) = current {
        let key = php_key(last);
        entries.retain(|(k, _)| *k != key);
    }
}

/// `array_replace_recursive( $base, $replacements )`: keys of `replacements` replace those of `base` (recursing
/// where both are arrays); new keys are appended; `base`'s order is kept.
pub fn replace_recursive(base: Value, replacements: Value) -> Value {
    let (Value::Array(mut base), Value::Array(replacements)) = (base, replacements) else {
        return Value::Null;
    };
    for (key, value) in replacements {
        match base.iter_mut().find(|(k, _)| *k == key) {
            Some(slot) => {
                let old = std::mem::replace(&mut slot.1, Value::Null);
                slot.1 = match (old, value) {
                    (old @ Value::Array(_), value @ Value::Array(_)) => {
                        replace_recursive(old, value)
                    }
                    (_, value) => value,
                };
            }
            None => base.push((key, value)),
        }
    }
    Value::Array(base)
}
