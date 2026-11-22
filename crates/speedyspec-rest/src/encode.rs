//! REST responses to HTTP responses, and neutral values as JSON.

use speedyspec_http::{Message, StatusCode, StreamFactory};
use speedyspec_http_message::{HttpResponse, MessageFactory};
use speedyspec_operation_contracts::Value;
use speedyspec_rest_contracts::RestResponse;

const BASE64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Standard base64, the JSON form of binary values.
pub fn base64(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for (i, shift) in [18, 12, 6, 0].into_iter().enumerate() {
            if i <= chunk.len() {
                out.push(BASE64[((n >> shift) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// A neutral value as JSON text, keeping map order. Binary is base64; dates are `Y-m-d\TH:i:s`.
pub fn json_text(value: &Value) -> String {
    let mut out = String::new();
    write_json(value, &mut out);
    out
}

fn write_json(value: &Value, out: &mut String) {
    let quote = |s: &str, out: &mut String| {
        out.push_str(&serde_json::to_string(s).unwrap_or_else(|_| "\"\"".into()))
    };
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Int(i) => out.push_str(&i.to_string()),
        Value::Float(f) if f.is_finite() => {
            out.push_str(&serde_json::Number::from_f64(*f).map_or("null".into(), |n| n.to_string()))
        }
        Value::Float(_) => out.push_str("null"),
        Value::String(s) => quote(s, out),
        Value::Binary(bytes) => quote(&base64(bytes), out),
        Value::DateTime(dt) => quote(&dt.format("%Y-%m-%dT%H:%M:%S").to_string(), out),
        Value::List(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_json(item, out);
            }
            out.push(']');
        }
        Value::Map(map) => {
            out.push('{');
            for (i, (k, v)) in map.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                quote(k, out);
                out.push(':');
                write_json(v, out);
            }
            out.push('}');
        }
    }
}

/// A neutral value as a `serde_json` value (tests and documents; map order is `serde_json`'s).
pub fn to_json(value: &Value) -> serde_json::Value {
    serde_json::from_str(&json_text(value)).unwrap_or(serde_json::Value::Null)
}

/// A `serde_json` value as a neutral value.
pub fn from_serde(json: &serde_json::Value) -> Value {
    crate::decode::from_json(json)
}

/// The HTTP response for `response`: a JSON body (none for `HEAD`), its headers, and a `Link` header for its links.
pub fn http_response(response: &RestResponse, head: bool) -> HttpResponse {
    let status = StatusCode::new(response.status)
        .unwrap_or_else(|_| StatusCode::new(500).expect("500 is a status"));
    let mut headers: Vec<(String, String)> = vec![(
        "Content-Type".into(),
        "application/json; charset=UTF-8".into(),
    )];
    headers.extend(response.headers.iter().map(|(k, v)| (k.clone(), v.clone())));
    if !response.links.is_empty() {
        let links: Vec<String> = response
            .links
            .iter()
            .map(|l| format!("<{}>; rel=\"{}\"", l.href, l.rel))
            .collect();
        headers.push(("Link".into(), links.join(", ")));
    }
    let mut http = HttpResponse::new(status);
    for (name, value) in headers {
        // A header HTTP cannot carry is skipped, never allowed to drop the response.
        let valid = !name.is_empty()
            && name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b))
            && !value.contains(['\r', '\n']);
        if valid && let Ok(with) = http.clone().with_header(&name, vec![value]) {
            http = with;
        }
    }
    let body = if head {
        Vec::new()
    } else {
        json_text(&response.data).into_bytes()
    };
    http.with_body(MessageFactory.create_stream(&body))
}
