//! The background-image slice of `wp_style_engine_get_styles( array( 'background' => … ) )`.

use speedyspec_hooks::Value;

use crate::array_path;
use crate::php_value::{as_str, empty};
use crate::presets::to_kebab_case;

/// `WP_Style_Engine::is_valid_style_value()`.
fn valid(value: &Value) -> bool {
    matches!(value, Value::Str(s) if s == "0") || !empty(value)
}

/// `get_url_or_value_css_declaration()` for `backgroundImage`.
fn image(value: &Value) -> Option<String> {
    if !valid(value) {
        return None;
    }
    if let Some(url) = array_path::get(value, &["url"]).filter(|u| !empty(u)) {
        return Some(format!(
            "url('{}')",
            String::from_utf8_lossy(&crate::php::to_php_string(url))
        ));
    }
    as_str(value).map(|s| s.into_owned())
}

/// `get_css_declarations()` for `gradient`, whose preset variable is `--wp--preset--gradient--$slug`.
fn gradient(value: &Value) -> Option<String> {
    if !valid(value) {
        return None;
    }
    if let Some(text) = as_str(value)
        && text.contains("var:")
    {
        if text.contains("var:preset|gradient|") {
            let slug = to_kebab_case(&text.as_bytes()[text.rfind('|').map_or(0, |i| i + 1)..]);
            if valid(&Value::Str(slug.clone())) {
                return Some(format!("var(--wp--preset--gradient--{slug})"));
            }
        }
        return None;
    }
    match value {
        Value::Array(_) => None,
        other => Some(String::from_utf8_lossy(&crate::php::to_php_string(other)).into_owned()),
    }
}

/// The `background-image` declaration the style engine writes for these values (each passed only when not
/// empty), or `None`: the gradient comes first when both are set.
pub fn background_image_value(
    background_image: Option<&Value>,
    gradient_value: Option<&Value>,
) -> Option<Value> {
    let mut declaration: Option<String> = background_image.and_then(image);
    if let Some(g) = gradient_value.and_then(gradient) {
        declaration = Some(match declaration {
            Some(existing) => format!("{g}, {existing}"),
            None => g,
        });
    }
    declaration.map(Value::Str)
}
