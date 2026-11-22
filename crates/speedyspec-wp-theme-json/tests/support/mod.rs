//! Helpers shared by the integration tests.

#![allow(dead_code)]

use speedyspec_hooks::{Key, Value};

/// A PHP value from JSON: objects become string-keyed arrays in the text's key order (stylesheets follow data
/// order), lists become lists, integers stay integers.
pub fn json(text: &str) -> Value {
    let mut reader = JsonReader {
        bytes: text.as_bytes(),
        at: 0,
    };
    let value = reader.value();
    reader.space();
    assert_eq!(reader.at, reader.bytes.len(), "trailing JSON");
    value
}

struct JsonReader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl JsonReader<'_> {
    fn space(&mut self) {
        while self.at < self.bytes.len() && self.bytes[self.at].is_ascii_whitespace() {
            self.at += 1;
        }
    }

    fn value(&mut self) -> Value {
        self.space();
        match self.bytes[self.at] {
            b'{' => {
                self.at += 1;
                let mut entries = Vec::new();
                loop {
                    self.space();
                    if self.bytes[self.at] == b'}' {
                        self.at += 1;
                        break;
                    }
                    let key = self.string();
                    self.space();
                    assert_eq!(self.bytes[self.at], b':');
                    self.at += 1;
                    let value = self.value();
                    let key = php_key(&key);
                    match entries
                        .iter_mut()
                        .find(|(k, _): &&mut (Key, Value)| *k == key)
                    {
                        Some(slot) => slot.1 = value,
                        None => entries.push((key, value)),
                    }
                    self.space();
                    if self.bytes[self.at] == b',' {
                        self.at += 1;
                    }
                }
                Value::Array(entries)
            }
            b'[' => {
                self.at += 1;
                let mut items = Vec::new();
                loop {
                    self.space();
                    if self.bytes[self.at] == b']' {
                        self.at += 1;
                        break;
                    }
                    items.push((Key::Int(items.len() as i64), self.value()));
                    self.space();
                    if self.bytes[self.at] == b',' {
                        self.at += 1;
                    }
                }
                Value::Array(items)
            }
            b'"' => Value::Str(self.string()),
            b't' => self.word("true", Value::Bool(true)),
            b'f' => self.word("false", Value::Bool(false)),
            b'n' => self.word("null", Value::Null),
            _ => self.number(),
        }
    }

    fn word(&mut self, word: &str, value: Value) -> Value {
        assert!(self.bytes[self.at..].starts_with(word.as_bytes()));
        self.at += word.len();
        value
    }

    fn number(&mut self) -> Value {
        let start = self.at;
        while self.at < self.bytes.len() && b"+-0123456789.eE".contains(&self.bytes[self.at]) {
            self.at += 1;
        }
        let text = std::str::from_utf8(&self.bytes[start..self.at]).unwrap();
        match serde_json::from_str::<serde_json::Number>(text).unwrap() {
            n if n.is_i64() => Value::Int(n.as_i64().unwrap()),
            n => Value::Float(n.as_f64().unwrap()),
        }
    }

    fn string(&mut self) -> String {
        let start = self.at;
        assert_eq!(self.bytes[self.at], b'"');
        self.at += 1;
        while self.bytes[self.at] != b'"' {
            if self.bytes[self.at] == b'\\' {
                self.at += 1;
            }
            self.at += 1;
        }
        self.at += 1;
        serde_json::from_slice(&self.bytes[start..self.at]).unwrap()
    }
}

/// PHP's array key normalisation: a canonical decimal integer string is an integer key.
pub fn php_key(k: &str) -> Key {
    match k.parse::<i64>() {
        Ok(i) if i.to_string() == k => Key::Int(i),
        _ => Key::Str(k.to_owned()),
    }
}

/// Asserts PHP's `==` between two values, printing both on failure.
#[track_caller]
pub fn assert_php_eq(actual: &Value, expected: &Value) {
    assert!(
        speedyspec_wp_theme_json::php_loose_eq(actual, expected),
        "actual:   {actual:?}\nexpected: {expected:?}"
    );
}

/// Asserts a string result is exactly WordPress's (byte for byte); other values compare with PHP's `==`.
#[track_caller]
pub fn assert_php_same(actual: &Value, expected: &Value) {
    match (actual, expected) {
        (Value::Str(a), Value::Str(e)) => assert_eq!(a, e),
        _ => assert_php_eq(actual, expected),
    }
}

/// WordPress's `null` or a string.
pub fn opt(value: Option<String>) -> Value {
    value.map_or(Value::Null, Value::Str)
}

/// A PHP string value.
pub fn st(value: impl Into<String>) -> Value {
    Value::Str(value.into())
}

/// Text functions close enough to WordPress's for slugs made of ASCII letters, digits and dashes.
pub struct AsciiText;

impl speedyspec_wp_theme_json_contracts::ports::TextFunctions for AsciiText {
    fn sanitize_title(&self, s: &[u8]) -> Vec<u8> {
        s.to_ascii_lowercase()
    }
    fn sanitize_html_class(&self, s: &[u8]) -> Vec<u8> {
        s.to_vec()
    }
    fn esc_html(&self, s: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        for b in s {
            match b {
                b'<' => out.extend_from_slice(b"&lt;"),
                b'>' => out.extend_from_slice(b"&gt;"),
                b'&' => out.extend_from_slice(b"&amp;"),
                b'"' => out.extend_from_slice(b"&quot;"),
                b'\'' => out.extend_from_slice(b"&#039;"),
                b => out.push(*b),
            }
        }
        out
    }
    fn esc_attr(&self, s: &[u8]) -> Vec<u8> {
        self.esc_html(s)
    }
}

/// A block as the registries report it, from JSON pieces.
pub fn block(
    name: &str,
    selectors: &str,
    supports: &str,
    styles: &[&str],
) -> speedyspec_wp_theme_json_contracts::ports::RegisteredBlock {
    speedyspec_wp_theme_json_contracts::ports::RegisteredBlock {
        name: name.into(),
        selectors: if selectors.is_empty() {
            Value::Array(Vec::new())
        } else {
            json(selectors)
        },
        supports: if supports.is_empty() {
            Value::Null
        } else {
            json(supports)
        },
        style_names: styles.iter().map(|s| s.to_string()).collect(),
    }
}

/// The ports for tests, over these blocks.
pub fn deps(
    blocks: Vec<speedyspec_wp_theme_json_contracts::ports::RegisteredBlock>,
) -> (
    speedyspec_wp_theme_json::ThemeJsonDeps,
    std::rc::Rc<speedyspec_wp_theme_json::RecordedDiagnostics>,
) {
    use std::rc::Rc;
    let registry = speedyspec_wp_theme_json::MemoryBlockMetadata::new();
    for b in blocks {
        registry.register(b);
    }
    let notices = Rc::new(speedyspec_wp_theme_json::RecordedDiagnostics::default());
    let deps = speedyspec_wp_theme_json::ThemeJsonDeps::new(
        Rc::new(registry),
        Rc::new(AsciiText),
        Rc::new(speedyspec_wp_theme_json::NoTranslation),
        notices.clone(),
        safe_css(),
    );
    (deps, notices)
}

/// A PHP list of values.
pub fn php_list(items: Vec<Value>) -> Value {
    Value::Array(
        items
            .into_iter()
            .enumerate()
            .map(|(i, v)| (Key::Int(i as i64), v))
            .collect(),
    )
}

struct NoEscape;
impl speedyspec_wp_kses_contracts::AttributeEscaper for NoEscape {
    fn esc_attr(&self, v: &[u8]) -> Result<Vec<u8>, speedyspec_wp_kses_contracts::KsesError> {
        Ok(v.to_vec())
    }
}
struct NoUnfiltered;
impl speedyspec_wp_kses_contracts::UnfilteredHtmlPolicy for NoUnfiltered {
    fn can_unfiltered_html(&self) -> Result<bool, speedyspec_wp_kses_contracts::KsesError> {
        Ok(false)
    }
}
struct Uploads;
impl speedyspec_wp_kses::UploadsLocation for Uploads {
    fn upload_url(&self) -> Result<Vec<u8>, speedyspec_wp_kses_contracts::KsesError> {
        Ok(b"http://example.org/wp-content/uploads".to_vec())
    }
}

/// kses's `safecss_filter_attr()`, the Rust port, over an empty Hook API (no `safe_style_css` filters).
pub fn safe_css() -> std::rc::Rc<dyn speedyspec_wp_kses_contracts::SafeCss> {
    use std::rc::Rc;
    let hooks = Rc::new(speedyspec_wp_hooks::HookApi::new(
        Rc::new(speedyspec_wp_hooks_memory::MemoryHookStore::new()),
        Rc::new(speedyspec_wp_hooks::IgnoreDeprecations),
    ));
    speedyspec_wp_kses::KsesApi::shared(speedyspec_wp_kses::KsesDeps {
        hooks,
        escaper: Rc::new(NoEscape),
        policy: Rc::new(NoUnfiltered),
        globals: Rc::new(speedyspec_wp_kses::DefaultGlobals::default()),
        uploads: Rc::new(Uploads),
        callbacks: Rc::new(speedyspec_wp_kses::NoValueCallbacks),
    })
}
