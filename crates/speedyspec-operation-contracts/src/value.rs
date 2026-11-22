//! The neutral value: what every transport decodes its wire format to, and encodes from.

use indexmap::IndexMap;

/// A JSON-like tree, extended with binary and datetime so XML-RPC `base64`/`dateTime.iso8601`, multipart uploads and
/// protobuf bytes survive between transports. Maps keep insertion order.
#[derive(Clone, Debug, PartialEq, Default)]
pub enum Value {
    #[default]
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    String(String),
    Binary(Vec<u8>),
    /// A date and time with no offset, UTC by convention.
    DateTime(chrono::NaiveDateTime),
    List(Vec<Value>),
    Map(IndexMap<String, Value>),
}

impl Value {
    /// A map from `entries`, in order.
    pub fn map<K: Into<String>>(entries: impl IntoIterator<Item = (K, Value)>) -> Value {
        Value::Map(entries.into_iter().map(|(k, v)| (k.into(), v)).collect())
    }

    /// The value under `key`, when this is a map that has it.
    pub fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Value::Map(map) => map.get(key),
            _ => None,
        }
    }

    /// The JSON Schema-style name of this value's kind.
    pub fn kind_name(&self) -> &'static str {
        match self {
            Value::Null => "null",
            Value::Bool(_) => "boolean",
            Value::Int(_) => "integer",
            Value::Float(_) => "number",
            Value::String(_) => "string",
            Value::Binary(_) => "binary",
            Value::DateTime(_) => "datetime",
            Value::List(_) => "array",
            Value::Map(_) => "object",
        }
    }
}

impl From<&str> for Value {
    fn from(value: &str) -> Self {
        Value::String(value.to_owned())
    }
}

impl From<String> for Value {
    fn from(value: String) -> Self {
        Value::String(value)
    }
}

impl From<i64> for Value {
    fn from(value: i64) -> Self {
        Value::Int(value)
    }
}

impl From<f64> for Value {
    fn from(value: f64) -> Self {
        Value::Float(value)
    }
}

impl From<bool> for Value {
    fn from(value: bool) -> Self {
        Value::Bool(value)
    }
}
