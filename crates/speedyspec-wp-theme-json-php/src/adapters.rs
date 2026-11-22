//! The ports, adapted onto WordPress's PHP: the block registries, the text and translation functions, and the
//! notice functions. Each calls the PHP function by name, so a native port of that function is used when loaded.

use std::cell::RefCell;
use std::rc::Rc;

use speedyspec_hooks::{Key, Value};
use speedyspec_wp_theme_json_contracts::ports::{
    BlockMetadata, Diagnostics, Gettext, RegisteredBlock, TextFunctions,
};

fn call(function: &str, args: &[Value]) -> Option<Value> {
    speedyspec_php::call(&Value::Str(function.into()), args).ok()
}

fn bytes_of(value: &Value) -> Option<Vec<u8>> {
    match value {
        Value::Str(s) => Some(s.as_bytes().to_vec()),
        Value::Bytes(b) => Some(b.clone()),
        _ => None,
    }
}

fn string_value(bytes: &[u8]) -> Value {
    match std::str::from_utf8(bytes) {
        Ok(s) => Value::Str(s.to_owned()),
        Err(_) => Value::Bytes(bytes.to_vec()),
    }
}

/// `WP_Block_Type_Registry` and `WP_Block_Styles_Registry`, read through the area's support functions.
///
/// WordPress caches block metadata in a static that only ever grows during a request (`get_blocks_metadata`), so
/// a block unregistered after its metadata was first read stays valid; this adapter keeps the same memory.
pub struct PhpBlockRegistry {
    seen: RefCell<Seen>,
}

/// What the adapter remembers between reads, as WordPress's `static::$blocks_metadata` does.
struct Seen {
    marker: Value,
    blocks: Rc<Vec<RegisteredBlock>>,
    /// The last read found new blocks, so (as in WordPress) it did not look at new styles of known blocks; the
    /// next read does, even if nothing changed since.
    styles_pending: bool,
    /// Counts the reads that changed the blocks (and the resets), so the change marker moves with them.
    generation: u64,
}

impl Default for PhpBlockRegistry {
    fn default() -> Self {
        PhpBlockRegistry {
            seen: RefCell::new(Seen {
                marker: Value::Null,
                blocks: Rc::new(Vec::new()),
                styles_pending: false,
                generation: 0,
            }),
        }
    }
}

impl PhpBlockRegistry {
    /// Forgets the blocks seen (a new request). The generation keeps counting, so metadata cached in an earlier
    /// request is never taken for this one's.
    pub fn reset(&self) {
        let mut seen = self.seen.borrow_mut();
        let generation = seen.generation + 1;
        *seen = Seen {
            marker: Value::Null,
            blocks: Rc::new(Vec::new()),
            styles_pending: false,
            generation,
        };
    }
}

fn strings(value: Option<&Value>) -> Vec<String> {
    match value {
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(|(_, v)| match v {
                Value::Str(s) => Some(s.clone()),
                Value::Int(i) => Some(i.to_string()),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

impl BlockMetadata for PhpBlockRegistry {
    /// WordPress's `get_blocks_metadata()` memory: blocks are never forgotten within a request, a block's
    /// selectors and supports are those it had when first seen, and new block styles of known blocks are taken
    /// only by a read that finds no new blocks.
    fn registered(&self) -> Rc<Vec<RegisteredBlock>> {
        let marker =
            call("SpeedySpec\\WpThemeJson\\Support\\block_marker", &[]).unwrap_or(Value::Null);
        {
            let seen = self.seen.borrow();
            if seen.marker == marker && !matches!(marker, Value::Null) && !seen.styles_pending {
                return seen.blocks.clone();
            }
        }
        let listed =
            call("SpeedySpec\\WpThemeJson\\Support\\registered_blocks", &[]).unwrap_or(Value::Null);
        let mut blocks = (*self.seen.borrow().blocks).clone();
        let mut entries: Vec<(String, Value, Value, Vec<String>)> = Vec::new();
        if let Value::Array(items) = listed {
            for (_, entry) in items {
                let Value::Array(fields) = entry else {
                    continue;
                };
                let field = |i: i64| {
                    fields
                        .iter()
                        .find(|(k, _)| *k == Key::Int(i))
                        .map(|(_, v)| v)
                };
                let Some(Value::Str(name)) = field(0) else {
                    continue;
                };
                entries.push((
                    name.clone(),
                    field(1).cloned().unwrap_or(Value::Null),
                    field(2).cloned().unwrap_or(Value::Null),
                    strings(field(3)),
                ));
            }
        }
        let has_new = entries
            .iter()
            .any(|(name, ..)| !blocks.iter().any(|b| b.name == *name));
        for (name, selectors, supports, styles) in entries {
            match blocks.iter_mut().find(|b| b.name == name) {
                Some(known) if !has_new => {
                    for style in styles {
                        if !known.style_names.contains(&style) {
                            known.style_names.push(style);
                        }
                    }
                }
                Some(_) => {}
                None => blocks.push(RegisteredBlock {
                    name,
                    selectors,
                    supports,
                    style_names: styles,
                }),
            }
        }
        let mut seen = self.seen.borrow_mut();
        let generation = if *seen.blocks == blocks {
            seen.generation
        } else {
            seen.generation + 1
        };
        let blocks = Rc::new(blocks);
        *seen = Seen {
            marker,
            blocks: blocks.clone(),
            styles_pending: has_new,
            generation,
        };
        blocks
    }

    fn change_marker(&self) -> u64 {
        let _ = self.registered();
        self.seen.borrow().generation
    }
}

/// The text functions, as PHP defines them (WordPress's `formatting.php`, or the native text port).
pub struct PhpTextFunctions;

impl PhpTextFunctions {
    fn through(function: &str, text: &[u8]) -> Vec<u8> {
        call(function, &[string_value(text)])
            .as_ref()
            .and_then(bytes_of)
            .unwrap_or_else(|| text.to_vec())
    }
}

impl TextFunctions for PhpTextFunctions {
    fn sanitize_title(&self, title: &[u8]) -> Vec<u8> {
        Self::through("sanitize_title", title)
    }

    fn sanitize_html_class(&self, classname: &[u8]) -> Vec<u8> {
        Self::through("sanitize_html_class", classname)
    }

    fn esc_html(&self, text: &[u8]) -> Vec<u8> {
        Self::through("esc_html", text)
    }

    fn esc_attr(&self, text: &[u8]) -> Vec<u8> {
        Self::through("esc_attr", text)
    }
}

/// `__()`, as PHP defines it.
pub struct PhpGettext;

impl Gettext for PhpGettext {
    fn translate(&self, text: &str, domain: &str) -> Vec<u8> {
        call("__", &[Value::Str(text.into()), Value::Str(domain.into())])
            .as_ref()
            .and_then(bytes_of)
            .unwrap_or_else(|| text.as_bytes().to_vec())
    }
}

/// WordPress's notice functions.
pub struct PhpDiagnostics;

impl Diagnostics for PhpDiagnostics {
    fn trigger_error(&self, function: &str, message: &[u8]) {
        call(
            "wp_trigger_error",
            &[
                Value::Str(function.into()),
                string_value(message),
                Value::Int(1024),
            ],
        );
    }

    fn deprecated_function(&self, function: &str, version: &str, replacement: &str) {
        call(
            "_deprecated_function",
            &[
                Value::Str(function.into()),
                Value::Str(version.into()),
                Value::Str(replacement.into()),
            ],
        );
    }

    fn doing_it_wrong(&self, function: &str, message: &[u8], version: &str) {
        call(
            "_doing_it_wrong",
            &[
                Value::Str(function.into()),
                string_value(message),
                Value::Str(version.into()),
            ],
        );
    }

    fn deprecated_argument(&self, function: &str, version: &str, message: &[u8]) {
        call(
            "_deprecated_argument",
            &[
                Value::Str(function.into()),
                Value::Str(version.into()),
                string_value(message),
            ],
        );
    }
}

/// kses's `safecss_filter_attr()`, as PHP defines it (WordPress's `kses.php`, or the native kses port).
pub struct PhpSafeCss;

impl speedyspec_wp_theme_json_contracts::ports::SafeCss for PhpSafeCss {
    fn safecss_filter_attr(
        &self,
        css: &[u8],
    ) -> Result<Vec<u8>, speedyspec_wp_kses_contracts::KsesError> {
        Ok(call("safecss_filter_attr", &[string_value(css)])
            .as_ref()
            .and_then(bytes_of)
            .unwrap_or_default())
    }
}

/// `wp_get_global_settings()`, as PHP defines it. Until the resolver is ported (plan 4) this is WordPress's own,
/// which calls back into this engine.
pub struct PhpGlobalSettings;

impl speedyspec_wp_theme_json_contracts::ports::GlobalSettings for PhpGlobalSettings {
    fn settings(&self) -> Value {
        match call("wp_get_global_settings", &[]) {
            Some(v) => match v.read().into_owned() {
                v @ Value::Array(_) => v,
                _ => Value::Array(Vec::new()),
            },
            None => Value::Array(Vec::new()),
        }
    }
}

/// `current_theme_supports()`, as PHP defines it.
pub struct PhpActiveTheme;

impl speedyspec_wp_theme_json_contracts::ports::ActiveTheme for PhpActiveTheme {
    fn supports(&self, feature: &str) -> bool {
        matches!(
            call("current_theme_supports", &[Value::Str(feature.into())]),
            Some(Value::Bool(true))
        )
    }
}
