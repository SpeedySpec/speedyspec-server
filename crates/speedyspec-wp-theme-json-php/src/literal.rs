//! PHP source literals for constant values, so the class constants are WordPress's exact arrays.

use speedyspec_hooks::{Key, Value};

fn string(bytes: &[u8]) -> String {
    let mut out = String::from("'");
    for &b in bytes {
        match b {
            b'\'' => out.push_str("\\'"),
            b'\\' => out.push_str("\\\\"),
            0x20..=0x7e => out.push(b as char),
            _ => {
                return format!(
                    "\"{}\"",
                    bytes
                        .iter()
                        .map(|c| format!("\\x{c:02x}"))
                        .collect::<String>()
                );
            }
        }
    }
    out.push('\'');
    out
}

/// `var_export`-style PHP source for a constant expression.
pub fn php_literal(value: &Value) -> String {
    match value {
        Value::Null => "null".into(),
        Value::Bool(b) => b.to_string(),
        Value::Int(i) => i.to_string(),
        Value::Float(f) => format!("{f:?}"),
        Value::Str(s) => string(s.as_bytes()),
        Value::Bytes(b) => string(b),
        Value::Array(entries) => {
            let items: Vec<String> = entries
                .iter()
                .map(|(k, v)| {
                    let key = match k {
                        Key::Int(i) => i.to_string(),
                        Key::Str(s) => string(s.as_bytes()),
                    };
                    format!("{key} => {}", php_literal(v))
                })
                .collect();
            format!("array({})", items.join(", "))
        }
        Value::Opaque(_) => "null".into(),
    }
}
