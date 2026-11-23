//! PHP values to and from neutral values, typed as IXR types them: `IXR_Date` is a date, `IXR_Base64` binary, an
//! object a struct of its properties, an array a list when keyed `0..n` in order and a struct otherwise.
//!
//! Values cross as tagged arrays (`SpeedySpec\XmlRpc\Internal\Adapter::tag` and `untag`), so dates and binary keep
//! their kinds and a struct keeps keys such as `"0"`.

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use indexmap::IndexMap;
use speedyspec_operation_contracts::Value;
use speedyspec_php_ext::{Key, Value as PhpValue};
use speedyspec_xmlrpc::codec::parse_date;

const ADAPTER: &str = "SpeedySpec\\XmlRpc\\Internal\\Adapter::";

/// Calls a static method of the PHP adapter; `None` (with the reason logged) when it fails.
pub(crate) fn adapter(method: &str, args: &[PhpValue]) -> Option<PhpValue> {
    speedyspec_php::call(&PhpValue::Str(format!("{ADAPTER}{method}")), args)
        .map_err(|err| {
            eprintln!("speedyspec_wordpress: the XML-RPC adapter's {method} failed: {err}");
        })
        .ok()
}

fn field<'a>(items: &'a [(Key, PhpValue)], name: &str) -> Option<&'a PhpValue> {
    items
        .iter()
        .find(|(k, _)| *k == Key::Str(name.into()))
        .map(|(_, v)| v)
}

fn text(value: Option<&PhpValue>) -> String {
    match value {
        Some(PhpValue::Str(s)) => s.clone(),
        Some(PhpValue::Bytes(b)) => String::from_utf8_lossy(b).into_owned(),
        Some(PhpValue::Int(i)) => i.to_string(),
        _ => String::new(),
    }
}

/// The neutral value a tagged array describes.
pub(crate) fn from_tagged(tagged: &PhpValue) -> Value {
    let tagged = tagged.read();
    let PhpValue::Array(items) = tagged.as_ref() else {
        return Value::Null;
    };
    let value = field(items, "v");
    match text(field(items, "t")).as_str() {
        "b" => Value::Bool(matches!(value, Some(PhpValue::Bool(true)))),
        "i" => match value {
            Some(PhpValue::Int(i)) => Value::Int(*i),
            _ => Value::Int(0),
        },
        "f" => match value {
            Some(PhpValue::Float(f)) => Value::Float(*f),
            Some(PhpValue::Int(i)) => Value::Float(*i as f64),
            _ => Value::Float(0.0),
        },
        "s" => Value::String(text(value)),
        "d" => {
            let iso = text(value);
            parse_date(&iso).map_or(Value::String(iso), Value::DateTime)
        }
        "x" => STANDARD
            .decode(text(value))
            .map_or(Value::Binary(Vec::new()), Value::Binary),
        "l" => match value {
            Some(PhpValue::Array(list)) => {
                Value::List(list.iter().map(|(_, v)| from_tagged(v)).collect())
            }
            _ => Value::List(Vec::new()),
        },
        "m" => {
            let mut map = IndexMap::new();
            if let Some(PhpValue::Array(pairs)) = value {
                for (_, pair) in pairs {
                    if let PhpValue::Array(pair) = pair
                        && let [(_, key), (_, item)] = pair.as_slice()
                    {
                        map.insert(text(Some(key)), from_tagged(item));
                    }
                }
            }
            Value::Map(map)
        }
        _ => Value::Null,
    }
}

fn str_key(name: &str, value: PhpValue) -> (Key, PhpValue) {
    (Key::Str(name.into()), value)
}

/// The tagged array for a neutral value.
pub(crate) fn to_tagged(value: &Value) -> PhpValue {
    let tag = |t: &str, v: Option<PhpValue>| {
        let mut items = vec![str_key("t", PhpValue::Str(t.into()))];
        if let Some(v) = v {
            items.push(str_key("v", v));
        }
        PhpValue::Array(items)
    };
    match value {
        Value::Null => tag("n", None),
        Value::Bool(b) => tag("b", Some(PhpValue::Bool(*b))),
        Value::Int(i) => tag("i", Some(PhpValue::Int(*i))),
        Value::Float(f) => tag("f", Some(PhpValue::Float(*f))),
        Value::String(s) => tag("s", Some(PhpValue::Str(s.clone()))),
        Value::DateTime(at) => tag(
            "d",
            Some(PhpValue::Str(at.format("%Y%m%dT%H:%M:%S").to_string())),
        ),
        Value::Binary(bytes) => tag("x", Some(PhpValue::Str(STANDARD.encode(bytes)))),
        Value::List(items) => tag(
            "l",
            Some(PhpValue::Array(
                items
                    .iter()
                    .enumerate()
                    .map(|(i, v)| (Key::Int(i as i64), to_tagged(v)))
                    .collect(),
            )),
        ),
        Value::Map(map) => tag(
            "m",
            Some(PhpValue::Array(
                map.iter()
                    .enumerate()
                    .map(|(i, (k, v))| {
                        (
                            Key::Int(i as i64),
                            PhpValue::Array(vec![
                                (Key::Int(0), PhpValue::Str(k.clone())),
                                (Key::Int(1), to_tagged(v)),
                            ]),
                        )
                    })
                    .collect(),
            )),
        ),
    }
}

/// The neutral value for a PHP value, as IXR would encode it.
pub fn to_neutral(value: &PhpValue) -> Value {
    adapter("tag", std::slice::from_ref(value)).map_or(Value::Null, |t| from_tagged(&t))
}

/// The PHP value for a neutral value: dates as `IXR_Date`, binary as `IXR_Base64` (plain strings when IXR is not
/// loaded).
pub fn to_php(value: &Value) -> PhpValue {
    adapter("untag", &[to_tagged(value)]).unwrap_or(PhpValue::Null)
}
