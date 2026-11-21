//! PHP values as the oracle encodes them, and `$allowed_html` arrays read with kses's own rules.

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use serde_json::{Value, json};
use speedyspec_wp_kses_contracts::{
    AllowedAttrs, AllowedHtml, AttrRule, AttrRules, ByteMap, Bytes, RuleValue, ValueCallback,
};

/// Encodes bytes as an oracle string.
pub fn bytes(v: &[u8]) -> Value {
    json!({ "b64": STANDARD.encode(v) })
}

/// Decodes an oracle string (or a plain JSON string) to bytes, casting other scalars as PHP does.
pub fn from_bytes(v: &Value) -> Vec<u8> {
    match v {
        Value::String(s) => s.as_bytes().to_vec(),
        Value::Object(o) => o
            .get("b64")
            .and_then(Value::as_str)
            .and_then(|s| STANDARD.decode(s).ok())
            .unwrap_or_default(),
        // PHP's string cast: an integer array key such as "1" comes back as a number, true as "1".
        Value::Number(n) => n.to_string().into_bytes(),
        Value::Bool(true) => b"1".to_vec(),
        _ => Vec::new(),
    }
}

fn pairs(v: &Value) -> Option<&Vec<Value>> {
    v.get("array").and_then(Value::as_array)
}

fn key(v: &Value) -> Bytes {
    match v {
        Value::Number(n) => n.to_string().into_bytes().into(),
        other => from_bytes(other).into(),
    }
}

fn is_string(v: &Value) -> bool {
    v.get("b64").is_some() || v.is_string()
}

/// A PHP scalar or list, as a rule's value.
pub fn rule_value(v: &Value) -> RuleValue {
    match v {
        Value::Null => RuleValue::Null,
        Value::Bool(b) => RuleValue::Bool(*b),
        Value::Number(n) => n
            .as_i64()
            .map(RuleValue::Int)
            .unwrap_or_else(|| RuleValue::Float(n.as_f64().unwrap_or(0.0))),
        v if is_string(v) => RuleValue::Str(from_bytes(v).into()),
        v => RuleValue::List(
            pairs(v)
                .map(|p| p.iter().map(|kv| rule_value(&kv[1])).collect())
                .unwrap_or_default(),
        ),
    }
}

/// PHP's `empty()`.
fn empty(v: &Value) -> bool {
    match v {
        Value::Null => true,
        Value::Bool(b) => !b,
        Value::Number(n) => n.as_f64() == Some(0.0),
        v if is_string(v) => matches!(&from_bytes(v)[..], b"" | b"0"),
        v => pairs(v).is_none_or(|p| p.is_empty()),
    }
}

fn rules(v: &Value, resolve: &dyn Fn(&[u8]) -> Option<ValueCallback>) -> AttrRules {
    match v {
        Value::Null => AttrRules::Denied,
        v if is_string(v) && from_bytes(v).is_empty() => AttrRules::Denied,
        v if pairs(v).is_some() => AttrRules::Rules(
            pairs(v)
                .into_iter()
                .flatten()
                .map(|kv| {
                    let name = key(&kv[0]);
                    let value = &kv[1];
                    match &name.to_ascii_lowercase()[..] {
                        b"maxlen" => AttrRule::MaxLen(rule_value(value)),
                        b"minlen" => AttrRule::MinLen(rule_value(value)),
                        b"maxval" => AttrRule::MaxVal(rule_value(value)),
                        b"minval" => AttrRule::MinVal(rule_value(value)),
                        b"valueless" => AttrRule::Valueless(rule_value(value)),
                        b"values" => match rule_value(value) {
                            RuleValue::List(list) => AttrRule::Values(list),
                            other => AttrRule::Other(name, other),
                        },
                        b"value_callback" if is_string(value) => {
                            match resolve(&from_bytes(value)) {
                                Some(callback) => AttrRule::ValueCallback(callback),
                                None => AttrRule::Other(name, rule_value(value)),
                            }
                        }
                        _ => AttrRule::Other(name, rule_value(value)),
                    }
                })
                .collect(),
        ),
        v if empty(v) => AttrRules::AllowedFalsy,
        _ => AttrRules::Allowed,
    }
}

/// Reads a PHP `$allowed_html` array: a `null` tag is absent (`isset`), `true` or `empty()` keeps the tag bare, an
/// array lists attributes, and any other scalar keeps the tag with every attribute denied. `resolve` turns a
/// `value_callback` name into a callback.
pub fn allowed_from_php(
    v: &Value,
    resolve: &dyn Fn(&[u8]) -> Option<ValueCallback>,
) -> AllowedHtml {
    let mut tags = ByteMap::default();
    for kv in pairs(v).into_iter().flatten() {
        let (name, value) = (key(&kv[0]), &kv[1]);
        let attrs = match value {
            Value::Null => continue,
            Value::Bool(true) => AllowedAttrs::Empty,
            v if empty(v) => AllowedAttrs::Empty,
            v => AllowedAttrs::Attrs(
                pairs(v)
                    .into_iter()
                    .flatten()
                    .map(|a| (key(&a[0]), rules(&a[1], resolve)))
                    .collect(),
            ),
        };
        tags.insert(name, attrs);
    }
    AllowedHtml { tags }
}
