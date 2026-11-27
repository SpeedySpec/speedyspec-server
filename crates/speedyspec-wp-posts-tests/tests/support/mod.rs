//! Shared by the tests: the hook API over the memory store, the `Formatting` test double over the Rust escaping,
//! the recorded compat/posts-read scenarios and their fixture callbacks, and the recorder's value encoding.

#![allow(dead_code)]

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use speedyspec_php_serialize::semantics::{is_integer_key, to_php_string};
use speedyspec_php_serialize::{PhpKey, PhpValue, from_hook_value, to_hook_value};
use speedyspec_wp_hooks::{IgnoreDeprecations, WpHookApi};
use speedyspec_wp_hooks_contracts::{Callback, Count, HookError, HookName, HookValue};
use speedyspec_wp_hooks_memory::MemoryHookStore;
use speedyspec_wp_posts::Formatting;
use speedyspec_wp_posts_contracts::PostError;
use speedyspec_wp_text_contracts::TextError;
use speedyspec_wp_text_escape::EscapeApi;
use speedyspec_wp_text_memory::{FixedSiteSettings, RecordingDiagnostics};

/// A fresh hook API over the memory store.
pub fn hooks() -> Rc<WpHookApi> {
    Rc::new(WpHookApi::new(
        Rc::new(MemoryHookStore::new()),
        Rc::new(IgnoreDeprecations),
    ))
}

fn text_error(e: TextError) -> PostError {
    PostError::Hook(HookError {
        message: e.message,
        payload: e.payload,
    })
}

/// The `Formatting` test double: WordPress's functions as the Rust escaping runs them, on the same hook API, with
/// `blog_charset` UTF-8.
///
/// `user_can_richedit()` starts from false, as it does in the recorder (no browser), then fires its filter.
/// `format_to_edit()` fires its filter, then `esc_textarea()` unless rich text.
pub struct RustFormatting {
    hooks: Rc<WpHookApi>,
    escape: EscapeApi,
}

impl RustFormatting {
    pub fn new(hooks: Rc<WpHookApi>) -> RustFormatting {
        let escape = EscapeApi::new(
            hooks.clone(),
            Rc::new(FixedSiteSettings::default()),
            Rc::new(RecordingDiagnostics::default()),
        );
        RustFormatting { hooks, escape }
    }

    fn filter(&self, hook: &str, value: HookValue) -> Result<HookValue, PostError> {
        let hook = HookName::from(hook);
        if self.hooks.skip_if_empty(&hook, Count::Filters)? {
            return Ok(value);
        }
        Ok(self.hooks.apply_filters(&hook, value, Vec::new())?)
    }
}

fn php(value: &HookValue) -> PhpValue {
    from_hook_value(value).expect("a PHP value")
}

impl Formatting for RustFormatting {
    fn format_to_edit(&self, content: PhpValue, rich_text: bool) -> Result<PhpValue, PostError> {
        let content = php(&self.filter("format_to_edit", to_hook_value(&content))?);
        if rich_text {
            return Ok(content);
        }
        let out = self
            .escape
            .esc_textarea(&to_php_string(&content))
            .map_err(text_error)?;
        Ok(php(&out))
    }

    fn esc_attr(&self, text: PhpValue) -> Result<PhpValue, PostError> {
        let out = self
            .escape
            .esc_attr(&to_php_string(&text))
            .map_err(text_error)?;
        Ok(php(&out))
    }

    fn esc_js(&self, text: PhpValue) -> Result<PhpValue, PostError> {
        let out = self
            .escape
            .esc_js(&to_php_string(&text))
            .map_err(text_error)?;
        Ok(php(&out))
    }

    fn user_can_richedit(&self) -> Result<bool, PostError> {
        let out = php(&self.filter("user_can_richedit", HookValue::Bool(false))?);
        Ok(!speedyspec_php_serialize::semantics::is_empty(&out))
    }
}

// ---- the recordings ----

fn compat(file: &str) -> serde_json::Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../compat/posts-read")
        .join(file);
    serde_json::from_str(&std::fs::read_to_string(&path).expect("the recording")).unwrap()
}

/// The scenarios whose name starts with `prefix`, each with its recorded steps, in `scenarios.json` order.
pub fn scenarios(prefix: &str) -> Vec<(String, serde_json::Value, Vec<serde_json::Value>)> {
    let defined = compat("scenarios.json");
    let recorded = compat("wordpress-7.2-RC2.json");
    defined["scenarios"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|s| s["name"].as_str().unwrap().starts_with(prefix))
        .map(|s| {
            let name = s["name"].as_str().unwrap().to_owned();
            let steps = recorded["results"][&name].as_array().unwrap().clone();
            (name, s.clone(), steps)
        })
        .collect()
}

/// A PHP array key as PHP stores a string key: canonical integers become int keys.
pub fn php_key(name: &str) -> PhpKey {
    if is_integer_key(name.as_bytes()) {
        PhpKey::Int(name.parse().unwrap())
    } else {
        PhpKey::Str(name.as_bytes().to_vec())
    }
}

fn recorded_key(key: &serde_json::Value) -> PhpKey {
    match key {
        serde_json::Value::Number(n) => PhpKey::Int(n.as_i64().unwrap()),
        serde_json::Value::String(s) => PhpKey::Str(s.as_bytes().to_vec()),
        other => panic!("an unrecorded key form {other}"),
    }
}

/// A value in the recorder's encoding (`SpeedySpecCompat::normalise()`): `{"array": [[k, v], …]}` for arrays,
/// scalars as they are. A `{"callback": label}` (a labelled object) decodes to `Err(label)`.
pub fn decode(value: &serde_json::Value) -> Result<PhpValue, String> {
    Ok(match value {
        serde_json::Value::Null => PhpValue::Null,
        serde_json::Value::Bool(b) => PhpValue::Bool(*b),
        serde_json::Value::Number(n) => PhpValue::Int(n.as_i64().expect("an int")),
        serde_json::Value::String(s) => PhpValue::Str(s.as_bytes().to_vec()),
        serde_json::Value::Object(map) if map.contains_key("array") => PhpValue::Array(
            map["array"]
                .as_array()
                .unwrap()
                .iter()
                .map(|pair| {
                    let pair = pair.as_array().unwrap();
                    (
                        recorded_key(&pair[0]),
                        decode(&pair[1]).expect("a plain value"),
                    )
                })
                .collect(),
        ),
        serde_json::Value::Object(map) if map.contains_key("callback") => {
            return Err(map["callback"].as_str().unwrap().to_owned());
        }
        other => panic!("an unrecorded value form {other}"),
    })
}

/// A recorded `kept:<name>` state entry (`{class, props}`) as an object.
pub fn decode_kept(value: &serde_json::Value) -> PhpValue {
    let PhpValue::Array(items) = decode(value).unwrap() else {
        panic!("a kept value is an array");
    };
    let field = |name: &str| {
        items
            .iter()
            .find(|(k, _)| *k == PhpKey::Str(name.as_bytes().to_vec()))
            .map(|(_, v)| v.clone())
            .unwrap()
    };
    let PhpValue::Str(class) = field("class") else {
        panic!("a class name");
    };
    let PhpValue::Array(properties) = field("props") else {
        panic!("properties");
    };
    PhpValue::Object {
        class: String::from_utf8(class).unwrap(),
        properties,
    }
}

/// The `kept:<name>` entries of a recorded step's state.
pub fn kept(step: &serde_json::Value) -> Vec<(String, PhpValue)> {
    let Some(state) = step["state"]["array"].as_array() else {
        return Vec::new();
    };
    state
        .iter()
        .filter_map(|pair| {
            let key = pair[0].as_str()?;
            let name = key.strip_prefix("kept:")?;
            Some((name.to_owned(), decode_kept(&pair[1])))
        })
        .collect()
}

/// One fixture callback's run: its label and the arguments it received.
pub type Invocations = Rc<RefCell<Vec<(String, PhpValue)>>>;

/// Adds a fixture callback as the driver's `SpeedySpecCompat::invoke()` runs it: `return_arg` (the argument at
/// `with.index`, 0 by default) and `append` (`with.suffix` after a string first argument). Each run is recorded.
pub fn add_fixture(
    hooks: &WpHookApi,
    log: &Invocations,
    hook: &str,
    spec: &serde_json::Value,
    priority: i64,
    accepted_args: i64,
) {
    let label = spec["label"].as_str().unwrap().to_owned();
    let kind = spec["callback"].as_str().unwrap().to_owned();
    let with = spec["with"].clone();
    let log = log.clone();
    let id = label.clone();
    let run = move |args: &[HookValue]| -> Result<HookValue, HookError> {
        let reported = PhpValue::Array(
            args.iter()
                .enumerate()
                .map(|(i, a)| (PhpKey::Int(i as i64), php(a)))
                .collect(),
        );
        log.borrow_mut().push((label.clone(), reported));
        match kind.as_str() {
            "return_arg" => {
                let index = with["index"].as_u64().unwrap_or(0) as usize;
                Ok(args.get(index).cloned().unwrap_or(HookValue::Null))
            }
            "append" => {
                let PhpValue::Str(mut first) = php(args.first().unwrap_or(&HookValue::Null)) else {
                    panic!("append is recorded on strings only");
                };
                first.extend_from_slice(with["suffix"].as_str().unwrap_or("").as_bytes());
                Ok(to_hook_value(&PhpValue::Str(first)))
            }
            other => panic!("fixture {other} is not recorded here"),
        }
    };
    hooks
        .add_filter(
            &HookName::from(hook),
            Callback::rust(&id, run),
            &HookValue::Int(priority),
            accepted_args,
        )
        .unwrap();
}

/// A recorded invocation as `(label, args)`.
pub fn recorded_invocations(step: &serde_json::Value) -> Vec<(String, PhpValue)> {
    step["invocations"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| {
            (
                i["fixture"].as_str().unwrap().to_owned(),
                decode(&i["args"]).unwrap(),
            )
        })
        .collect()
}
