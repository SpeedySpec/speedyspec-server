use speedyspec_routing::{QueryValue, QueryVars};

use super::php::php_urldecode;

/// PHP's `parse_str`: `&`-separated pairs, URL-decoded, `a[]` and `a[b]` names nested, spaces and dots in the base
/// name turned into `_`, the last value winning while keeping the first position.
pub fn php_parse_str(query: &str) -> QueryVars {
    let mut vars: QueryVars = Vec::new();
    for pair in query.split('&').filter(|pair| !pair.is_empty()) {
        let (name, value) = match pair.split_once('=') {
            Some((name, value)) => (php_urldecode(name), php_urldecode(value)),
            None => (php_urldecode(pair), String::new()),
        };
        let name = name.trim_start_matches(' ');
        if name.is_empty() {
            continue;
        }
        let (base, keys) = split_name(name);
        if base.is_empty() {
            continue;
        }
        insert(&mut vars, &base, &keys, value);
    }
    vars
}

/// Splits `a.b[c][]` into `a_b` and `["c", ""]`, with PHP's mangling of the base name.
fn split_name(name: &str) -> (String, Vec<String>) {
    let mut base = String::new();
    let mut keys = Vec::new();
    let chars: Vec<char> = name.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '[' {
            if chars[i + 1..].contains(&']') {
                let mut j = i;
                while j < chars.len() && chars[j] == '[' {
                    let Some(close) = chars[j + 1..].iter().position(|&c| c == ']') else {
                        break;
                    };
                    keys.push(chars[j + 1..j + 1 + close].iter().collect());
                    j += close + 2;
                }
                return (base, keys);
            }
            base.push('_');
        } else if c == ' ' || c == '.' {
            base.push('_');
        } else {
            base.push(c);
        }
        i += 1;
    }
    (base, keys)
}

fn insert(vars: &mut QueryVars, key: &str, keys: &[String], value: String) {
    let Some((next, rest)) = keys.split_first() else {
        match vars.iter_mut().find(|(existing, _)| existing == key) {
            Some(entry) => entry.1 = QueryValue::Text(value),
            None => vars.push((key.to_string(), QueryValue::Text(value))),
        }
        return;
    };
    let position = match vars.iter().position(|(existing, _)| existing == key) {
        Some(i) => {
            if !matches!(vars[i].1, QueryValue::Map(_)) {
                vars[i].1 = QueryValue::Map(Vec::new());
            }
            i
        }
        None => {
            vars.push((key.to_string(), QueryValue::Map(Vec::new())));
            vars.len() - 1
        }
    };
    let QueryValue::Map(map) = &mut vars[position].1 else {
        return;
    };
    let next = if next.is_empty() {
        let max = map.iter().filter_map(|(k, _)| k.parse::<i64>().ok()).max();
        max.map_or(0, |m| m + 1).to_string()
    } else {
        next.clone()
    };
    insert(map, &next, rest, value);
}
