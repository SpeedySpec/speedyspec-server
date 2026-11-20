//! `serialize()`.

use crate::float::php_float;
use crate::{PhpKey, PhpValue};

fn string(out: &mut Vec<u8>, tag: &str, bytes: &[u8]) {
    out.extend_from_slice(format!("{tag}:{}:\"", bytes.len()).as_bytes());
    out.extend_from_slice(bytes);
    out.push(b'"');
}

fn key(out: &mut Vec<u8>, k: &PhpKey) {
    match k {
        PhpKey::Int(i) => out.extend_from_slice(format!("i:{i};").as_bytes()),
        PhpKey::Str(s) => {
            string(out, "s", s);
            out.push(b';');
        }
    }
}

fn members(out: &mut Vec<u8>, items: &[(PhpKey, PhpValue)]) {
    out.extend_from_slice(format!("{}:{{", items.len()).as_bytes());
    for (k, v) in items {
        key(out, k);
        write(out, v);
    }
    out.push(b'}');
}

fn write(out: &mut Vec<u8>, v: &PhpValue) {
    match v {
        PhpValue::Null => out.extend_from_slice(b"N;"),
        PhpValue::Bool(b) => out.extend_from_slice(if *b { b"b:1;" } else { b"b:0;" }),
        PhpValue::Int(i) => out.extend_from_slice(format!("i:{i};").as_bytes()),
        PhpValue::Float(f) => out.extend_from_slice(format!("d:{};", php_float(*f)).as_bytes()),
        PhpValue::Str(s) => {
            string(out, "s", s);
            out.push(b';');
        }
        PhpValue::Array(items) => {
            out.extend_from_slice(b"a:");
            members(out, items);
        }
        PhpValue::Object { class, properties } => {
            string(out, "O", class.as_bytes());
            out.push(b':');
            members(out, properties);
        }
        PhpValue::Custom { class, data } => {
            string(out, "C", class.as_bytes());
            out.extend_from_slice(format!(":{}:{{", data.len()).as_bytes());
            out.extend_from_slice(data);
            out.push(b'}');
        }
        PhpValue::Enum { class, case } => {
            string(out, "E", format!("{class}:{case}").as_bytes());
            out.push(b';');
        }
        PhpValue::Ref(n) => out.extend_from_slice(format!("R:{n};").as_bytes()),
        PhpValue::ObjectRef(n) => out.extend_from_slice(format!("r:{n};").as_bytes()),
    }
}

/// PHP's `serialize()`.
pub fn serialize(value: &PhpValue) -> Vec<u8> {
    let mut out = Vec::new();
    write(&mut out, value);
    out
}
