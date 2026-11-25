//! Shared by the recorded tests: finds `compat/posts/cases.json` and decodes its `{"t","v"}` values.

use std::path::PathBuf;

use serde_json::{Value, json};
use speedyspec_php_serialize::semantics::{key_to_string, to_array, to_array_key, to_php_string};
use speedyspec_php_serialize::{PhpKey, PhpValue};
use speedyspec_wp_posts::{
    NAME_LENGTH_VERSION, PostTypePorts, PostTypeRegistry, Untranslated, check_post_type_name,
};
use speedyspec_wp_posts_contracts::{PostTypeError, RewriteContext, TitleSanitizer};

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

/// An action as the recorder writes it: `[hook, callback, priority, accepted_args]`.
pub type Action = (PhpValue, PhpValue, i64, i64);

/// What one replayed call gave, in the recording's terms.
#[allow(dead_code)]
pub struct Replayed {
    /// The function's return value.
    pub result: PhpValue,
    /// `_doing_it_wrong()` calls, as `{function, message, version}`.
    pub doing_it_wrong: Value,
    /// Callbacks the call hooked that were not hooked before.
    pub actions: Vec<Action>,
    /// Callbacks the call unhooked that were hooked before, with their accepted args.
    pub actions_removed: Vec<Action>,
    /// `register_taxonomy_for_object_type()` calls, as `[taxonomy, object_type]`.
    pub taxonomies: Value,
}

/// A `WP_Error` as [`decode`] gives it.
#[allow(dead_code)]
pub fn wp_error(error: PostTypeError) -> PhpValue {
    PhpValue::Object {
        class: "WP_Error".to_owned(),
        properties: vec![
            (
                PhpKey::Str(b"code".to_vec()),
                PhpValue::Str(error.code().as_bytes().to_vec()),
            ),
            (
                PhpKey::Str(b"message".to_vec()),
                PhpValue::Str(error.message().as_bytes().to_vec()),
            ),
        ],
    }
}

/// `$array[ $value ]`'s key as Rust keys post types and features.
fn key_text(value: &PhpValue) -> String {
    key_to_string(&to_array_key(value).expect("a scalar key"))
}

/// `$wp_filter` as the recorder's diff sees it: `(hook, priority, callback)` to accepted args, in insertion order.
#[derive(Default)]
struct Hooks(Vec<((PhpValue, i64, PhpValue), i64)>);

impl Hooks {
    fn add(
        &mut self,
        hook: &str,
        callback: PhpValue,
        priority: i64,
        accepted: i64,
    ) -> Option<Action> {
        let key = (PhpValue::Str(hook.as_bytes().to_vec()), priority, callback);
        if self.0.iter().any(|(k, _)| *k == key) {
            return None;
        }
        self.0.push((key.clone(), accepted));
        Some((key.0, key.2, priority, accepted))
    }

    fn remove(&mut self, hook: &str, callback: PhpValue, priority: i64) -> Option<Action> {
        let key = (PhpValue::Str(hook.as_bytes().to_vec()), priority, callback);
        let at = self.0.iter().position(|(k, _)| *k == key)?;
        let (key, accepted) = self.0.remove(at);
        Some((key.0, key.2, priority, accepted))
    }
}

/// Replays a `post_types` scenario on one [`PostTypeRegistry`], calling `each` with every recorded call, what the
/// registry answered and the registry after it; returns the registry at the end.
#[allow(dead_code)]
pub fn replay(
    scenario: &Value,
    mut each: impl FnMut(&Value, Replayed, &PostTypeRegistry),
) -> PostTypeRegistry {
    let ports = ports(!scenario["permalinks"].as_str().unwrap().is_empty());
    let mut registry = PostTypeRegistry::new();
    let mut hooks = Hooks::default();
    for call in scenario["calls"].as_array().unwrap() {
        let PhpValue::Array(args) = decode(&call["args"]) else {
            panic!("args is an array");
        };
        let args: Vec<PhpValue> = args.into_iter().map(|(_, v)| v).collect();
        let mut replayed = Replayed {
            result: PhpValue::Null,
            doing_it_wrong: json!([]),
            actions: Vec::new(),
            actions_removed: Vec::new(),
            taxonomies: json!([]),
        };
        match call["fn"].as_str().unwrap() {
            "register_post_type" => {
                let type_args = match args.get(1) {
                    Some(PhpValue::Array(items)) => items.clone(),
                    None => Vec::new(),
                    Some(other) => panic!("unrecorded args {other:?}"),
                };
                match check_post_type_name(&to_php_string(&args[0])) {
                    Err(error) => {
                        replayed.doing_it_wrong = json!([{
                            "function": "register_post_type",
                            "message": error.message(),
                            "version": NAME_LENGTH_VERSION,
                        }]);
                        replayed.result = wp_error(error);
                    }
                    Ok(name) => {
                        let registration = registry.register(name.clone(), type_args, &ports);
                        replayed.actions = registration
                            .actions
                            .into_iter()
                            .filter_map(|a| {
                                hooks.add(
                                    &a.hook,
                                    a.callback,
                                    a.priority,
                                    i64::from(a.accepted_args),
                                )
                            })
                            .collect();
                        replayed.taxonomies = Value::Array(
                            registration
                                .taxonomies
                                .iter()
                                .map(|t| {
                                    json!([
                                        String::from_utf8(to_php_string(t)).unwrap(),
                                        name.as_str()
                                    ])
                                })
                                .collect(),
                        );
                        replayed.result = registration.post_type.to_php();
                    }
                }
            }
            "unregister_post_type" => {
                replayed.result = match registry.unregister(&key_text(&args[0])) {
                    Ok(unregistration) => {
                        replayed.actions_removed = unregistration
                            .actions
                            .into_iter()
                            .filter_map(|a| hooks.remove(&a.hook, a.callback, a.priority))
                            .collect();
                        PhpValue::Bool(true)
                    }
                    Err(error) => wp_error(error),
                };
            }
            "add_post_type_support" => {
                let features: Vec<String> = to_array(args[1].clone())
                    .iter()
                    .map(|(_, f)| key_text(f))
                    .collect();
                let features: Vec<&str> = features.iter().map(String::as_str).collect();
                registry.add_support(&key_text(&args[0]), &features, args[2..].to_vec());
            }
            "remove_post_type_support" => {
                registry.remove_support(&key_text(&args[0]), &key_text(&args[1]));
            }
            "post_type_supports" => {
                use speedyspec_wp_posts_contracts::PostTypeSupports;
                replayed.result =
                    PhpValue::Bool(registry.supports(&key_text(&args[0]), &key_text(&args[1])));
            }
            "get_all_post_type_supports" => {
                replayed.result = PhpValue::Array(
                    registry
                        .all_supports(&key_text(&args[0]))
                        .into_iter()
                        .map(|(f, v)| (PhpKey::Str(f.into_bytes()), v))
                        .collect(),
                );
            }
            other => panic!("unrecorded function {other}"),
        }
        each(call, replayed, &registry);
    }
    registry
}

/// A recorded `[hook, callback, priority, accepted_args]` list.
#[allow(dead_code)]
pub fn recorded_actions(value: &Value) -> Vec<Action> {
    value
        .as_array()
        .unwrap()
        .iter()
        .map(|a| {
            (
                decode(&a[0]),
                decode(&a[1]),
                a[2].as_i64().unwrap(),
                a[3].as_i64().unwrap(),
            )
        })
        .collect()
}
