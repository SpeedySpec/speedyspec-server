//! Shared by the recorded tests: finds `compat/meta/cases.json` and decodes its `{"t","v"}` values.

use std::path::PathBuf;

use serde_json::Value;
use speedyspec_php_serialize::{PhpKey, PhpValue};

#[allow(dead_code)]
pub fn cases() -> Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../compat/meta/cases.json");
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

/// Decodes an encoded value: `null`, `bool`, `int`, `float`, `str` (hex) or `array` (`[[key, value], ...]`).
#[allow(dead_code)]
pub fn decode(value: &Value) -> PhpValue {
    let v = &value["v"];
    match value["t"].as_str().expect("encoded value has a type") {
        "null" => PhpValue::Null,
        "bool" => PhpValue::Bool(v.as_bool().unwrap()),
        "int" => PhpValue::Int(v.as_i64().unwrap()),
        "float" => PhpValue::Float(v.as_f64().unwrap()),
        "str" => PhpValue::Str(hex::decode(v.as_str().unwrap()).unwrap()),
        "array" => PhpValue::Array(
            v.as_array()
                .unwrap()
                .iter()
                .map(|pair| (decode_key(&pair[0]), decode(&pair[1])))
                .collect(),
        ),
        other => panic!("unknown encoded type {other}"),
    }
}

/// An array key is an encoded `int` or `str`.
#[allow(dead_code)]
pub fn decode_key(value: &Value) -> PhpKey {
    match decode(value) {
        PhpValue::Int(i) => PhpKey::Int(i),
        PhpValue::Str(s) => PhpKey::Str(s),
        _ => panic!("array keys are int or str"),
    }
}
