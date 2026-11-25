//! The pinned WordPress release's default allowlists, kept once per thread as the PHP-shaped values the filters receive, and the
//! helpers kses.php builds them with.

use serde_json::Value as Json;
use speedyspec_hooks::{Key, Value};

/// The WordPress release `data/allowed-<release>.json` was generated from. `include_str!` takes only a literal, so this
/// macro is the one place the crate names the release; a test checks that it equals the kses area's release
/// (`compat/kses/source.json`'s override, else `compat/wordpress.json`'s pin). Moving the pin edits this macro too.
macro_rules! data_release {
    () => {
        "7.2-RC2"
    };
}

/// `$allowedposttags` (after `_wp_add_global_attributes`) and `$allowedtags`, as pairs so order survives JSON.
const DATA: &str = include_str!(concat!("../data/allowed-", data_release!(), ".json"));

fn from_json(v: &Json) -> Value {
    match v {
        Json::Null => Value::Null,
        Json::Bool(b) => Value::Bool(*b),
        Json::Number(n) => n
            .as_i64()
            .map_or_else(|| Value::Float(n.as_f64().unwrap_or(0.0)), Value::Int),
        Json::String(s) => Value::Str(s.clone()),
        Json::Object(o) => Value::Array(
            o.get("pairs")
                .and_then(Json::as_array)
                .into_iter()
                .flatten()
                .map(|kv| {
                    let key = match &kv[0] {
                        Json::Number(n) => Key::Int(n.as_i64().unwrap_or(0)),
                        k => Key::Str(k.as_str().unwrap_or_default().to_string()),
                    };
                    (key, from_json(&kv[1]))
                })
                .collect(),
        ),
        Json::Array(_) => Value::Null,
    }
}

thread_local! {
    static DEFAULTS: (Value, Value) = {
        let data: Json = serde_json::from_str(DATA).expect("the bundled allowlists are valid JSON");
        (from_json(&data["allowedposttags"]), from_json(&data["allowedtags"]))
    };
}

/// `$allowedposttags` as the pinned WordPress release sets it up.
pub fn default_post_value() -> Value {
    DEFAULTS.with(|d| d.0.clone())
}

/// `$allowedtags` as the pinned WordPress release sets it up.
pub fn default_tags_value() -> Value {
    DEFAULTS.with(|d| d.1.clone())
}

/// The attributes `_wp_add_global_attributes()` adds to every post tag.
const GLOBAL_ATTRIBUTES: &[&str] = &[
    "aria-controls",
    "aria-current",
    "aria-describedby",
    "aria-details",
    "aria-expanded",
    "aria-hidden",
    "aria-label",
    "aria-labelledby",
    "aria-live",
    "class",
    "data-*",
    "dir",
    "hidden",
    "id",
    "lang",
    "style",
    "tabindex",
    "title",
    "role",
    "xml:lang",
];

/// `_wp_add_global_attributes()`: `true` becomes an array, and every array gets the global attributes as
/// `array_merge()` adds them (string keys overwrite in place, integer keys renumber).
pub fn add_global_attributes(value: &Value) -> Value {
    let base = match value {
        Value::Bool(true) => Vec::new(),
        Value::Array(items) => items.clone(),
        other => return other.clone(),
    };
    let globals = GLOBAL_ATTRIBUTES
        .iter()
        .map(|n| (Key::Str((*n).to_string()), Value::Bool(true)));
    let mut out: Vec<(Key, Value)> = Vec::with_capacity(base.len() + GLOBAL_ATTRIBUTES.len());
    let mut next = 0i64;
    for (k, v) in base.into_iter().chain(globals) {
        match k {
            Key::Int(_) => {
                out.push((Key::Int(next), v));
                next += 1;
            }
            Key::Str(s) => match out
                .iter_mut()
                .find(|(k, _)| matches!(k, Key::Str(t) if *t == s))
            {
                Some(slot) => slot.1 = v,
                None => out.push((Key::Str(s), v)),
            },
        }
    }
    Value::Array(out)
}

/// `parse_url()`'s host and `:port` of a URL, as `_wp_kses_allow_pdf_objects()` reads the uploads URL.
fn host_and_port(url: &[u8]) -> (Vec<u8>, Vec<u8>) {
    let Some(scheme_end) = memchr::memmem::find(url, b"://") else {
        return (Vec::new(), Vec::new());
    };
    let rest = &url[scheme_end + 3..];
    let authority = &rest[..rest
        .iter()
        .position(|b| b"/?#".contains(b))
        .unwrap_or(rest.len())];
    let authority = match memchr::memrchr(b'@', authority) {
        Some(at) => &authority[at + 1..],
        None => authority,
    };
    match memchr::memrchr(b':', authority) {
        Some(colon)
            if authority[colon + 1..].iter().all(u8::is_ascii_digit)
                && colon + 1 < authority.len() =>
        {
            (
                authority[..colon].to_vec(),
                [b":", &authority[colon + 1..]].concat(),
            )
        }
        _ => (authority.to_vec(), Vec::new()),
    }
}

/// `_wp_kses_allow_pdf_objects()`: a PDF from the uploads host, with no query or fragment.
pub fn allow_pdf_objects(url: &[u8], upload_url: &[u8]) -> bool {
    if url.contains(&b'?') || url.contains(&b'#') || !url.ends_with(b".pdf") {
        return false;
    }
    let (host, port) = host_and_port(upload_url);
    let prefix = |scheme: &[u8]| [scheme, b"://", &host, &port, b"/"].concat();
    url.starts_with(&prefix(b"http")) || url.starts_with(&prefix(b"https"))
}

#[cfg(test)]
mod release {
    #[test]
    fn the_allowlists_come_from_the_kses_areas_release() {
        let compat = concat!(env!("CARGO_MANIFEST_DIR"), "/../../compat");
        let json = |path: String| -> serde_json::Value {
            serde_json::from_str(&std::fs::read_to_string(&path).expect(&path)).expect(&path)
        };
        let source = json(format!("{compat}/kses/source.json"));
        let table = json(format!("{compat}/wordpress.json"));
        let release = source["wordpress"].as_str().or(table["wordpress"].as_str());
        assert_eq!(
            release,
            Some(data_release!()),
            "the kses area is recorded from WordPress {release:?}, not {}: regenerate \
             data/allowed-<release>.json with scripts/generate-allowed.php and change data_release!",
            data_release!()
        );
    }
}
