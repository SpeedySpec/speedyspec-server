//! Query strings and form bodies, read with PHP's `parse_str` rules.

use indexmap::IndexMap;
use speedyspec_operation_contracts::Value;

/// Percent-decodes `raw`, reading `+` as a space; invalid escapes are kept as they are.
pub fn decode_component(raw: &str) -> String {
    decode(raw, true)
}

/// Percent-decodes a URL path: `+` stays `+`.
pub fn decode_path(raw: &str) -> String {
    decode(raw, false)
}

fn decode(raw: &str, plus_is_space: bool) -> String {
    let bytes = raw.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' if plus_is_space => out.push(b' '),
            b'%' if i + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3])
                    .ok()
                    .and_then(|h| u8::from_str_radix(h, 16).ok());
                match hex {
                    Some(b) => {
                        out.push(b);
                        i += 2;
                    }
                    None => out.push(b'%'),
                }
            }
            b => out.push(b),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The segments of a PHP parameter name: `a[b][]` is `a`, `b`, `` (append).
fn segments(name: &str) -> Vec<String> {
    let Some(open) = name.find('[') else {
        return vec![name.to_owned()];
    };
    let mut parts = vec![name[..open].to_owned()];
    let mut rest = &name[open..];
    while let Some(stripped) = rest.strip_prefix('[') {
        let Some(close) = stripped.find(']') else {
            break;
        };
        parts.push(stripped[..close].to_owned());
        rest = &stripped[close + 1..];
    }
    parts
}

fn insert(target: &mut Value, path: &[String], value: Value) {
    let Some((first, rest)) = path.split_first() else {
        *target = value;
        return;
    };
    if first.is_empty() {
        if !matches!(target, Value::List(_)) {
            *target = Value::List(Vec::new());
        }
        let Value::List(list) = target else { return };
        let mut slot = Value::Null;
        insert(&mut slot, rest, value);
        list.push(slot);
        return;
    }
    if !matches!(target, Value::Map(_)) {
        *target = Value::Map(IndexMap::new());
    }
    let Value::Map(map) = target else { return };
    let slot = map.entry(first.clone()).or_insert(Value::Null);
    insert(slot, rest, value);
}

/// `raw` as PHP's `parse_str` reads it: `a[]=1&a[]=2` is a list, `a[b]=1` a map, a repeated plain key keeps the last
/// value. Values are strings.
pub fn parse(raw: &str) -> IndexMap<String, Value> {
    let mut root = Value::Map(IndexMap::new());
    for pair in raw.split('&').filter(|p| !p.is_empty()) {
        let (name, value) = pair.split_once('=').unwrap_or((pair, ""));
        let name = decode_component(name);
        if name.is_empty() || name.starts_with('[') {
            continue;
        }
        insert(
            &mut root,
            &segments(&name),
            Value::String(decode_component(value)),
        );
    }
    match root {
        Value::Map(map) => map,
        _ => IndexMap::new(),
    }
}
