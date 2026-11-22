//! Conversions between the neutral value and `serde_json` values (enum values in schemas).

use serde_json::Value as Json;
use speedyspec_operation_contracts::Value;

pub(crate) fn to_json(value: &Value) -> Json {
    match value {
        Value::Null => Json::Null,
        Value::Bool(b) => Json::Bool(*b),
        Value::Int(i) => Json::from(*i),
        Value::Float(f) => serde_json::Number::from_f64(*f).map_or(Json::Null, Json::Number),
        Value::String(s) => Json::String(s.clone()),
        Value::Binary(bytes) => Json::Array(bytes.iter().map(|b| Json::from(*b)).collect()),
        Value::DateTime(dt) => Json::String(dt.format("%Y-%m-%dT%H:%M:%S").to_string()),
        Value::List(items) => Json::Array(items.iter().map(to_json).collect()),
        Value::Map(map) => Json::Object(map.iter().map(|(k, v)| (k.clone(), to_json(v))).collect()),
    }
}

pub(crate) fn from_json(json: &Json) -> Value {
    match json {
        Json::Null => Value::Null,
        Json::Bool(b) => Value::Bool(*b),
        Json::Number(n) => match n.as_i64() {
            Some(i) => Value::Int(i),
            None => Value::Float(n.as_f64().unwrap_or(f64::NAN)),
        },
        Json::String(s) => Value::String(s.clone()),
        Json::Array(items) => Value::List(items.iter().map(from_json).collect()),
        Json::Object(map) => {
            Value::Map(map.iter().map(|(k, v)| (k.clone(), from_json(v))).collect())
        }
    }
}
