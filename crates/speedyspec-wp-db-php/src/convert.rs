//! Conversions at the PHP boundary.

use speedyspec_php_ext::{Key, Value};
use speedyspec_wp_db::{ColLength, Field, LengthKind, OutputShape, WpRow};

pub(crate) fn bytes_value(b: Vec<u8>) -> Value {
    match String::from_utf8(b) {
        Ok(s) => Value::Str(s),
        Err(e) => Value::Bytes(e.into_bytes()),
    }
}

pub(crate) fn opt_bytes(b: Option<Vec<u8>>) -> Value {
    b.map_or(Value::Null, bytes_value)
}

/// A PHP string's bytes; other scalars as PHP casts them.
pub(crate) fn bytes(v: &Value) -> Option<Vec<u8>> {
    match v {
        Value::Str(s) => Some(s.as_bytes().to_vec()),
        Value::Bytes(b) => Some(b.clone()),
        Value::Int(i) => Some(i.to_string().into_bytes()),
        Value::Float(f) => Some(speedyspec_wp_db::float_to_string(*f).into_bytes()),
        Value::Bool(true) => Some(b"1".to_vec()),
        Value::Bool(false) | Value::Null => Some(Vec::new()),
        _ => None,
    }
}

pub(crate) fn string(v: &Value) -> String {
    bytes(v)
        .map(|b| String::from_utf8_lossy(&b).into_owned())
        .unwrap_or_default()
}

pub(crate) fn int(v: &Value) -> i64 {
    match v {
        Value::Int(i) => *i,
        Value::Bool(b) => i64::from(*b),
        Value::Float(f) => *f as i64,
        other => string(other).trim().parse::<i64>().unwrap_or(0),
    }
}

pub(crate) fn truthy(v: &Value) -> bool {
    match v {
        Value::Null | Value::Bool(false) | Value::Int(0) => false,
        Value::Float(f) => *f != 0.0,
        Value::Array(a) => !a.is_empty(),
        Value::Str(s) => !s.is_empty() && s != "0",
        Value::Bytes(b) => !b.is_empty() && b != b"0",
        _ => true,
    }
}

pub(crate) fn list(items: Vec<Value>) -> Value {
    Value::Array(
        items
            .into_iter()
            .enumerate()
            .map(|(i, v)| (Key::Int(i as i64), v))
            .collect(),
    )
}

pub(crate) fn key_string(k: &Key) -> String {
    match k {
        Key::Int(i) => i.to_string(),
        Key::Str(s) => s.clone(),
    }
}

/// A PHP `array( 'name' => value )` as data for a helper; `None` for a non-array (WordPress returns false).
pub(crate) fn data(v: &Value) -> Option<Vec<(String, Value)>> {
    match v {
        Value::Array(items) => Some(
            items
                .iter()
                .map(|(k, v)| (key_string(k), v.clone()))
                .collect(),
        ),
        _ => None,
    }
}

/// A helper's `$format`: null, one string, or a list.
pub(crate) fn formats(v: &Value) -> Option<Vec<String>> {
    match v {
        Value::Null => None,
        Value::Array(items) => Some(items.iter().map(|(_, v)| string(v)).collect()),
        other => Some(vec![string(other)]),
    }
}

/// A row as `get_object_vars()` of its `mysqli_fetch_object` gives it (`ARRAY_A`).
pub(crate) fn row_assoc(row: &WpRow) -> Value {
    Value::Array(
        row.fields
            .iter()
            .map(|(n, v)| (Key::Str(n.clone()), opt_bytes(v.clone())))
            .collect(),
    )
}

/// A row as a list (`ARRAY_N`).
pub(crate) fn row_list(row: &WpRow) -> Value {
    list(
        row.fields
            .iter()
            .map(|(_, v)| opt_bytes(v.clone()))
            .collect(),
    )
}

/// `OBJECT`, `OBJECT_K`, `ARRAY_A`, `ARRAY_N`, or any case of `object`.
pub(crate) fn shape(v: &Value) -> Option<OutputShape> {
    OutputShape::parse(&string(v))
}

fn length_value(l: Option<ColLength>) -> Value {
    match l {
        None => Value::Bool(false),
        Some(l) => Value::Array(vec![
            (
                Key::Str("type".into()),
                Value::Str(if l.kind == LengthKind::Byte {
                    "byte".into()
                } else {
                    "char".into()
                }),
            ),
            (
                Key::Str("length".into()),
                Value::Int(i64::try_from(l.length).unwrap_or(i64::MAX)),
            ),
        ]),
    }
}

pub(crate) fn col_length(l: Option<ColLength>) -> Value {
    length_value(l)
}

fn length_of(v: Option<&Value>) -> Option<ColLength> {
    let Some(Value::Array(items)) = v else {
        return None;
    };
    let get = |name: &str| {
        items
            .iter()
            .find(|(k, _)| key_string(k) == name)
            .map(|(_, v)| v.clone())
    };
    let kind = if string(&get("type")?) == "byte" {
        LengthKind::Byte
    } else {
        LengthKind::Char
    };
    Some(ColLength {
        kind,
        length: u64::try_from(int(&get("length")?)).unwrap_or(0),
    })
}

/// `process_fields`' arrays: `[field => ['value' => .., 'format' => .., 'charset' => .., 'length' => ..]]`.
pub(crate) fn fields_to_php(fields: &[Field]) -> Value {
    Value::Array(
        fields
            .iter()
            .map(|f| {
                let mut entry = vec![
                    (Key::Str("value".into()), f.value.clone()),
                    (Key::Str("format".into()), Value::Str(f.format.clone())),
                ];
                entry.push((
                    Key::Str("charset".into()),
                    f.charset.clone().map_or(Value::Bool(false), Value::Str),
                ));
                entry.push((Key::Str("length".into()), length_value(f.length)));
                (Key::Str(f.name.clone()), Value::Array(entry))
            })
            .collect(),
    )
}

pub(crate) fn fields_from_php(v: &Value) -> Vec<Field> {
    let Value::Array(items) = v else {
        return Vec::new();
    };
    items
        .iter()
        .map(|(k, entry)| {
            let get = |name: &str| match entry {
                Value::Array(e) => e
                    .iter()
                    .find(|(k, _)| key_string(k) == name)
                    .map(|(_, v)| v.clone()),
                _ => None,
            };
            Field {
                name: key_string(k),
                value: get("value").unwrap_or(Value::Null),
                format: get("format")
                    .map(|v| string(&v))
                    .unwrap_or_else(|| "%s".into()),
                charset: get("charset")
                    .filter(|c| !matches!(c, Value::Bool(false) | Value::Null))
                    .map(|c| string(&c)),
                length: length_of(get("length").as_ref()),
            }
        })
        .collect()
}
