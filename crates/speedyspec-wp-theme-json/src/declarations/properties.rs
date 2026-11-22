//! `compute_style_properties` and `get_property_value`.

use speedyspec_hooks::{Key, Value};
use speedyspec_wp_theme_json_contracts::schema::{self, ROOT_BLOCK_SELECTOR};

use super::DeclarationContext;
use super::background::background_image_value;
use crate::array_path;
use crate::css::Declaration;
use crate::php::{json_encode, to_number, to_php_string};
use crate::php_value::{as_str, empty, entries, key_str, list, s};

/// `get_property_value( $styles, $path, $theme_json )`: the value at `path`, following one `ref` into the
/// document; `""` when there is none.
pub fn property_value(
    styles: &Value,
    path: &[String],
    theme_json: Option<&Value>,
    ctx: DeclarationContext<'_>,
) -> Value {
    let refs: Vec<&str> = path.iter().map(String::as_str).collect();
    let value = match array_path::get(styles, &refs) {
        None | Some(Value::Null) => return s(""),
        Some(Value::Str(t)) if t.is_empty() => return s(""),
        Some(v) => v.clone(),
    };
    let Some(reference) = array_path::get(&value, &["ref"]).filter(|r| !matches!(r, Value::Null))
    else {
        return value;
    };
    let reference = String::from_utf8_lossy(&to_php_string(reference)).into_owned();
    let ref_path: Vec<&str> = reference.split('.').collect();
    let ref_value = theme_json
        .and_then(|doc| array_path::get(doc, &ref_path))
        .cloned()
        .unwrap_or(Value::Null);
    let ref_url_is_string = matches!(ref_value, Value::Array(_))
        && array_path::get(&ref_value, &["url"])
            .and_then(as_str)
            .is_some();
    let mut result = value;
    if !empty(&ref_value) && (as_str(&ref_value).is_some() || ref_url_is_string) {
        result = ref_value.clone();
    }
    if let Some(inner) = array_path::get(&ref_value, &["ref"]).filter(|r| !matches!(r, Value::Null))
    {
        let path_string = json_encode(&list(path.iter().map(|p| s(p.clone())).collect()));
        let ref_value_string = json_encode(&ref_value);
        let template = ctx.gettext.translate(
            "Your %1$s file uses a dynamic value (%2$s) for the path at %3$s. However, the value at %3$s is also a dynamic value (pointing to %4$s) and pointing to another dynamic value is not supported. Please update %3$s to point directly to %4$s.",
            "default",
        );
        let message = String::from_utf8_lossy(&template)
            .replace("%1$s", "theme.json")
            .replace("%2$s", &ref_value_string)
            .replace("%3$s", &path_string)
            .replace("%4$s", &String::from_utf8_lossy(&to_php_string(inner)));
        ctx.diagnostics
            .doing_it_wrong("get_property_value", message.as_bytes(), "6.1.0");
    }
    result
}

fn path_of(value: &Value) -> Option<Vec<String>> {
    match value {
        Value::Array(steps) => Some(
            steps
                .iter()
                .map(|(_, v)| String::from_utf8_lossy(&to_php_string(v)).into_owned())
                .collect(),
        ),
        _ => None,
    }
}

/// `compute_style_properties( $styles, $settings, $properties, $theme_json, $selector, $use_root_padding )`:
/// the declarations for a style node; `properties` defaults to `PROPERTIES_METADATA`.
pub fn compute_style_properties(
    styles: &Value,
    settings: &Value,
    properties: Option<&Value>,
    theme_json: Option<&Value>,
    selector: Option<&str>,
    use_root_padding: bool,
    ctx: DeclarationContext<'_>,
) -> Vec<Declaration> {
    if empty(styles) {
        return Vec::new();
    }
    let default_properties;
    let properties = match properties {
        Some(p) => p,
        None => {
            default_properties = schema::constant("PROPERTIES_METADATA").unwrap_or(Value::Null);
            &default_properties
        }
    };
    let is_root_selector = selector == Some(ROOT_BLOCK_SELECTOR);
    let mut declarations: Vec<Declaration> = Vec::new();
    let mut root_variable_duplicates: Vec<String> = Vec::new();
    for (css_property, value_path) in entries(properties) {
        let Some(value_path) = path_of(value_path) else {
            continue;
        };
        let css_property = key_str(css_property);
        let is_root_style = css_property.starts_with("--wp--style--root--");
        if is_root_style && (!is_root_selector || !use_root_padding) {
            continue;
        }
        let mut value = property_value(styles, &value_path, theme_json, ctx);
        if css_property == "--wp--style--root--padding" && as_str(&value).is_some() {
            continue;
        }
        if is_root_style && use_root_padding {
            root_variable_duplicates.push(css_property["--wp--style--root--".len()..].to_owned());
        }
        if css_property == "background-image" {
            let image = (!empty(&value)).then_some(&value);
            let gradient =
                array_path::get(styles, &["background", "gradient"]).filter(|g| !empty(g));
            if image.is_some() || gradient.is_some() {
                value = background_image_value(image, gradient).unwrap_or(Value::Null);
            }
        }
        if empty(&value)
            && !is_root_selector
            && array_path::get(styles, &["background", "backgroundImage", "id"])
                .is_some_and(|id| !empty(id))
        {
            if css_property == "background-size" {
                value = s("cover");
            }
            if css_property == "background-position" {
                let size = array_path::get(styles, &["background", "backgroundSize"]);
                value = if matches!(size, Some(Value::Str(t)) if t == "contain") {
                    s("50% 50%")
                } else {
                    Value::Null
                };
            }
        }
        let missing = empty(&value) && to_number(&value).is_none();
        if missing || matches!(value, Value::Array(_)) {
            continue;
        }
        if css_property == "font-size" {
            value = ctx.fluid.value(
                &Value::Array(vec![(Key::Str("size".into()), value)]),
                settings,
            );
        }
        if css_property == "aspect-ratio" {
            declarations.push(Declaration::text("min-height", "unset"));
        }
        declarations.push(Declaration::new(css_property, value));
    }
    for duplicate in root_variable_duplicates {
        if let Some(index) = declarations.iter().position(|d| d.name == duplicate) {
            declarations.remove(index);
        }
    }
    declarations
}
