//! `$allowed_html` between its PHP shape (a hook value) and [`AllowedHtml`], with kses's own `isset`, `empty` and
//! `is_array` rules.

use std::rc::Rc;

use speedyspec_hooks::{Key, Value};
use speedyspec_wp_kses_contracts::{
    AllowedAttrs, AllowedHtml, AttrRule, AttrRules, ByteMap, Bytes, KsesError, RuleValue,
    ValueCallback,
};

fn key_bytes(k: &Key) -> Bytes {
    match k {
        Key::Int(i) => i.to_string().into_bytes().into(),
        Key::Str(s) => s.as_bytes().into(),
    }
}

fn str_bytes(v: &Value) -> Option<&[u8]> {
    match v {
        Value::Str(s) => Some(s.as_bytes()),
        Value::Bytes(b) => Some(b),
        _ => None,
    }
}

/// PHP's `empty()`.
fn empty(v: &Value) -> bool {
    match v {
        Value::Null => true,
        Value::Bool(b) => !b,
        Value::Int(i) => *i == 0,
        Value::Float(f) => *f == 0.0,
        Value::Str(_) | Value::Bytes(_) => matches!(str_bytes(v), Some(b"" | b"0")),
        Value::Array(items) => items.is_empty(),
        Value::Opaque(_) => false,
    }
}

/// A PHP scalar or list as an attribute rule's value.
pub fn rule_value(v: &Value) -> RuleValue {
    match v {
        Value::Null | Value::Opaque(_) => RuleValue::Null,
        Value::Bool(b) => RuleValue::Bool(*b),
        Value::Int(i) => RuleValue::Int(*i),
        Value::Float(f) => RuleValue::Float(*f),
        Value::Str(_) | Value::Bytes(_) => RuleValue::Str(str_bytes(v).unwrap_or_default().into()),
        Value::Array(items) => RuleValue::List(items.iter().map(|(_, v)| rule_value(v)).collect()),
    }
}

/// One check of an attribute's rule array, `name => value` as PHP keys it (`maxlen`, `values`, ...).
pub fn attr_rule(
    name: &[u8],
    value: &Value,
    resolve: &dyn Fn(&Value) -> Option<ValueCallback>,
) -> AttrRule {
    let name: Bytes = name.into();
    match &name.to_ascii_lowercase()[..] {
        b"maxlen" => AttrRule::MaxLen(rule_value(value)),
        b"minlen" => AttrRule::MinLen(rule_value(value)),
        b"maxval" => AttrRule::MaxVal(rule_value(value)),
        b"minval" => AttrRule::MinVal(rule_value(value)),
        b"valueless" => AttrRule::Valueless(rule_value(value)),
        // Not a list: PHP's `in_array()` stops with a TypeError; kses fails closed (no value matches).
        b"values" => match rule_value(value) {
            RuleValue::List(list) => AttrRule::Values(list),
            _ => AttrRule::Values(Vec::new()),
        },
        // Nothing can call it: PHP's `call_user_func()` stops with a TypeError; kses fails closed (rejects).
        b"value_callback" => match resolve(value) {
            Some(callback) => AttrRule::ValueCallback(callback),
            None => {
                let reject: ValueCallback = Rc::new(|_: &[u8]| Ok(false));
                AttrRule::ValueCallback(reject)
            }
        },
        _ => AttrRule::Other(name, rule_value(value)),
    }
}

/// One tag's attribute array (`$allowed_html[ $tag ]`) as kses reads it; a non-array has no attributes.
pub fn attr_rules_from_value(
    v: &Value,
    resolve: &dyn Fn(&Value) -> Option<ValueCallback>,
) -> ByteMap<AttrRules> {
    match &*v.read() {
        Value::Array(attrs) => attrs
            .iter()
            .map(|(k, a)| (key_bytes(k), rules(a, resolve)))
            .collect(),
        _ => ByteMap::default(),
    }
}

fn rules(v: &Value, resolve: &dyn Fn(&Value) -> Option<ValueCallback>) -> AttrRules {
    match v {
        Value::Null => AttrRules::Denied,
        v if str_bytes(v) == Some(b"") => AttrRules::Denied,
        Value::Array(items) => AttrRules::Rules(
            items
                .iter()
                .map(|(k, value)| attr_rule(&key_bytes(k), value, resolve))
                .collect(),
        ),
        v if empty(v) => AttrRules::AllowedFalsy,
        _ => AttrRules::Allowed,
    }
}

/// Reads a PHP `$allowed_html` array: a `null` tag is absent (`isset`), `true` or an `empty()` value keeps the tag
/// bare, an array lists attributes, any other value keeps the tag with every attribute denied. `resolve` turns a
/// `value_callback` into a callback.
pub fn allowed_from_value(
    v: &Value,
    resolve: &dyn Fn(&Value) -> Option<ValueCallback>,
) -> Result<AllowedHtml, KsesError> {
    let v = v.read();
    let mut tags = ByteMap::default();
    let Value::Array(items) = &*v else {
        return Ok(AllowedHtml { tags });
    };
    for (k, value) in items {
        let attrs = match value {
            Value::Null => continue,
            Value::Bool(true) => AllowedAttrs::Empty,
            v if empty(v) => AllowedAttrs::Empty,
            Value::Array(attrs) => AllowedAttrs::Attrs(
                attrs
                    .iter()
                    .map(|(k, a)| (key_bytes(k), rules(a, resolve)))
                    .collect(),
            ),
            _ => AllowedAttrs::Attrs(ByteMap::default()),
        };
        tags.insert(key_bytes(k), attrs);
    }
    Ok(AllowedHtml { tags })
}

fn key(name: &[u8]) -> Key {
    let text = String::from_utf8_lossy(name);
    match text.parse::<i64>() {
        Ok(i) if i.to_string() == text => Key::Int(i),
        _ => Key::Str(text.into_owned()),
    }
}

fn value_of(r: &RuleValue) -> Value {
    match r {
        RuleValue::Null => Value::Null,
        RuleValue::Bool(b) => Value::Bool(*b),
        RuleValue::Int(i) => Value::Int(*i),
        RuleValue::Float(f) => Value::Float(*f),
        RuleValue::Str(s) => match std::str::from_utf8(s) {
            Ok(t) => Value::Str(t.to_string()),
            Err(_) => Value::Bytes(s.to_vec()),
        },
        RuleValue::List(l) => Value::Array(
            l.iter()
                .enumerate()
                .map(|(i, v)| (Key::Int(i as i64), value_of(v)))
                .collect(),
        ),
    }
}

/// An [`AllowedHtml`] in its PHP shape, for a filter that has listeners. A bare tag becomes an empty array, and a
/// callback an opaque value Rust listeners can carry but PHP cannot call.
pub fn allowed_to_value(allowed: &AllowedHtml) -> Value {
    Value::Array(
        allowed
            .tags
            .iter()
            .map(|(tag, attrs)| {
                let attrs = match attrs {
                    AllowedAttrs::Empty => Value::Array(Vec::new()),
                    AllowedAttrs::Attrs(map) => Value::Array(
                        map.iter()
                            .map(|(name, rules)| {
                                let v = match rules {
                                    AttrRules::Denied => Value::Str(String::new()),
                                    AttrRules::Allowed => Value::Bool(true),
                                    AttrRules::AllowedFalsy => Value::Bool(false),
                                    AttrRules::Rules(list) => Value::Array(
                                        list.iter()
                                            .map(|rule| match rule {
                                                AttrRule::MaxLen(v) => {
                                                    (key(b"maxlen"), value_of(v))
                                                }
                                                AttrRule::MinLen(v) => {
                                                    (key(b"minlen"), value_of(v))
                                                }
                                                AttrRule::MaxVal(v) => {
                                                    (key(b"maxval"), value_of(v))
                                                }
                                                AttrRule::MinVal(v) => {
                                                    (key(b"minval"), value_of(v))
                                                }
                                                AttrRule::Valueless(v) => {
                                                    (key(b"valueless"), value_of(v))
                                                }
                                                AttrRule::Values(l) => (
                                                    key(b"values"),
                                                    value_of(&RuleValue::List(l.clone())),
                                                ),
                                                AttrRule::ValueCallback(c) => (
                                                    key(b"value_callback"),
                                                    Value::Opaque(Rc::new(c.clone())),
                                                ),
                                                AttrRule::Other(k, v) => (key(k), value_of(v)),
                                            })
                                            .collect(),
                                    ),
                                };
                                (key(name), v)
                            })
                            .collect(),
                    ),
                };
                (key(tag), attrs)
            })
            .collect(),
    )
}
