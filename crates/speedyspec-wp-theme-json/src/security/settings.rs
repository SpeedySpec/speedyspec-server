//! `remove_insecure_settings` and `preserve_valid_typed_settings`.

use speedyspec_hooks::{Key, Value};
use speedyspec_wp_theme_json_contracts::Origin;
use speedyspec_wp_theme_json_contracts::schema::{self, ValueFunc, presets_metadata};

use super::Security;
use crate::array_path;
use crate::php_value::{entries, key_str};
use crate::sanitize::viewport::sanitize_viewport_settings;

/// `preserve_valid_typed_settings( $input, &$output, $schema, $path )`: every boolean the schema marks as boolean.
fn preserve_valid_typed_settings(
    input: &Value,
    output: &mut Value,
    schema: &Value,
    path: &mut Vec<String>,
) {
    for (key, schema_value) in entries(schema) {
        path.push(key_str(key));
        match schema_value {
            Value::Bool(_) => {
                let refs: Vec<&str> = path.iter().map(String::as_str).collect();
                if let Some(value @ Value::Bool(_)) = array_path::get(input, &refs) {
                    array_path::set(output, &refs, value.clone());
                }
            }
            Value::Array(_) => preserve_valid_typed_settings(input, output, schema_value, path),
            _ => {}
        }
        path.pop();
    }
}

impl Security<'_> {
    /// `remove_insecure_settings( $input, $is_root )`.
    pub(crate) fn remove_insecure_settings(&self, input: &Value, is_root: bool) -> Value {
        let mut output = Value::Array(Vec::new());
        let text = &*self.deps.text;
        for metadata in presets_metadata().iter() {
            for origin in Origin::ALL {
                let mut path: Vec<&str> = metadata.path.iter().map(String::as_str).collect();
                path.push(origin.as_str());
                let Some(presets) =
                    array_path::get(input, &path).filter(|v| !matches!(v, Value::Null))
                else {
                    continue;
                };
                let mut escaped = Vec::new();
                for (_, preset) in entries(presets) {
                    let name_ok = match array_path::get(preset, &["name"]) {
                        Some(Value::Str(name)) => {
                            text.esc_attr(&text.esc_html(name.as_bytes())) == name.as_bytes()
                        }
                        _ => false,
                    };
                    let slug_ok = match array_path::get(preset, &["slug"]) {
                        Some(Value::Str(slug)) => {
                            text.sanitize_html_class(slug.as_bytes()) == slug.as_bytes()
                        }
                        _ => false,
                    };
                    if !(name_ok && slug_ok) {
                        continue;
                    }
                    let value = match (&metadata.value_key, metadata.value_func) {
                        (Some(key), _) if array_path::isset(preset, &[key.as_str()]) => {
                            array_path::get(preset, &[key.as_str()])
                                .cloned()
                                .unwrap_or(Value::Null)
                        }
                        (_, Some(ValueFunc::FontSize)) => {
                            self.fluid.value(preset, &Value::Array(Vec::new()))
                        }
                        _ => Value::Null,
                    };
                    if metadata
                        .properties
                        .iter()
                        .all(|property| self.safe(property, &value))
                    {
                        escaped.push(preset.clone());
                    }
                }
                if !escaped.is_empty() {
                    let list = Value::Array(
                        escaped
                            .into_iter()
                            .enumerate()
                            .map(|(i, p)| (Key::Int(i as i64), p))
                            .collect(),
                    );
                    array_path::set(&mut output, &path, list);
                }
            }
        }
        self.remove_indirect_properties(input, &mut output);
        preserve_valid_typed_settings(
            input,
            &mut output,
            &schema::table("VALID_SETTINGS"),
            &mut Vec::new(),
        );
        if is_root
            && matches!(input, Value::Array(e) if e.iter().any(|(k, _)| *k == Key::Str("viewport".into())))
        {
            let viewport = array_path::get(input, &["viewport"]);
            array_path::insert(
                &mut output,
                Key::Str("viewport".into()),
                sanitize_viewport_settings(viewport),
            );
        }
        output
    }
}
