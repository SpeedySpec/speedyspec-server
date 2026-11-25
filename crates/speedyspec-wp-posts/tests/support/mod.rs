//! Shared by the recorded tests: finds `compat/posts/cases.json` and decodes its `{"t","v"}` values.

use std::path::PathBuf;

use serde_json::Value;
use speedyspec_php_serialize::{PhpKey, PhpValue};
use speedyspec_wp_posts::{PostTypePorts, Untranslated};
use speedyspec_wp_posts_contracts::{RewriteContext, TitleSanitizer};

/// The recorded cases. The file is large: read it once per test.
#[allow(dead_code)]
pub fn cases() -> Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../compat/posts/cases.json");
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

/// Decodes an encoded value: `null`, `bool`, `int`, `float`, `str` (hex), `array` (`[[key, value], ...]`),
/// `object` (`class` and `[[property, value], ...]`) or `wp_error` (a `WP_Error` object with `code` and
/// `message` properties).
#[allow(dead_code)]
pub fn decode(value: &Value) -> PhpValue {
    let v = &value["v"];
    let pairs = |v: &Value| -> Vec<(PhpKey, PhpValue)> {
        v.as_array()
            .unwrap()
            .iter()
            .map(|pair| (decode_key(&pair[0]), decode(&pair[1])))
            .collect()
    };
    match value["t"].as_str().expect("encoded value has a type") {
        "null" => PhpValue::Null,
        "bool" => PhpValue::Bool(v.as_bool().unwrap()),
        "int" => PhpValue::Int(v.as_i64().unwrap()),
        "float" => PhpValue::Float(v.as_f64().unwrap()),
        "str" => PhpValue::Str(hex::decode(v.as_str().unwrap()).unwrap()),
        "array" => PhpValue::Array(pairs(v)),
        "object" => PhpValue::Object {
            class: value["class"].as_str().unwrap().to_owned(),
            properties: pairs(v),
        },
        "wp_error" => PhpValue::Object {
            class: "WP_Error".to_owned(),
            properties: vec![
                (
                    PhpKey::Str(b"code".to_vec()),
                    PhpValue::Str(value["code"].as_str().unwrap().as_bytes().to_vec()),
                ),
                (
                    PhpKey::Str(b"message".to_vec()),
                    PhpValue::Str(value["message"].as_str().unwrap().as_bytes().to_vec()),
                ),
            ],
        },
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

/// Top-level int keys (or property names) as their decimal strings, the way Rust keys post types, statuses and
/// features. Lossless: PHP turns only canonical integer strings into int keys.
#[allow(dead_code)]
pub fn normalize_keys(value: PhpValue) -> PhpValue {
    let normalize = |items: Vec<(PhpKey, PhpValue)>| {
        items
            .into_iter()
            .map(|(key, value)| match key {
                PhpKey::Int(i) => (PhpKey::Str(i.to_string().into_bytes()), value),
                key => (key, value),
            })
            .collect()
    };
    match value {
        PhpValue::Array(items) => PhpValue::Array(normalize(items)),
        PhpValue::Object { class, properties } => PhpValue::Object {
            class,
            properties: normalize(properties),
        },
        other => other,
    }
}

/// `$object->name` (or `$array['name']`), when present.
#[allow(dead_code)]
pub fn property<'a>(value: &'a PhpValue, name: &str) -> Option<&'a PhpValue> {
    let items = match value {
        PhpValue::Array(items) => items,
        PhpValue::Object { properties, .. } => properties,
        _ => return None,
    };
    items
        .iter()
        .find(|(k, _)| matches!(k, PhpKey::Str(s) if s.as_slice() == name.as_bytes()))
        .map(|(_, v)| v)
}

/// `is_admin() || get_option( 'permalink_structure' )`, fixed.
#[allow(dead_code)]
pub struct Permalinks(pub bool);

impl RewriteContext for Permalinks {
    fn rewrite_applies(&self) -> bool {
        self.0
    }
}

/// Enough of `sanitize_title_with_dashes()` for the recordings: ASCII-lowercase, whitespace to `-`, bytes outside
/// `[a-z0-9_-]` dropped, runs of `-` collapsed and trimmed (`My Books!` becomes `my-books`).
#[allow(dead_code)]
pub struct Dashes;

impl TitleSanitizer for Dashes {
    fn sanitize_title_with_dashes(&self, title: &[u8]) -> Vec<u8> {
        let mut out: Vec<u8> = Vec::with_capacity(title.len());
        for byte in title.to_ascii_lowercase() {
            let byte = if byte.is_ascii_whitespace() {
                b'-'
            } else {
                byte
            };
            if !(byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_' || byte == b'-')
            {
                continue;
            }
            if byte == b'-' && out.last() == Some(&b'-') {
                continue;
            }
            out.push(byte);
        }
        while out.last() == Some(&b'-') {
            out.pop();
        }
        while out.first() == Some(&b'-') {
            out.remove(0);
        }
        out
    }
}

/// The ports `set_props()` reads through: WordPress's English, the fixed rewrite context and [`Dashes`].
#[allow(dead_code)]
pub fn ports(permalinks: bool) -> PostTypePorts<'static> {
    PostTypePorts {
        translator: &Untranslated,
        rewrite: if permalinks {
            &Permalinks(true)
        } else {
            &Permalinks(false)
        },
        titles: &Dashes,
    }
}
