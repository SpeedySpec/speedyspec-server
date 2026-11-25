//! What a style state (a breakpoint, a pseudo selector or a custom state) needs to escape a text clip set on the
//! styles it layers over (WordPress 7.2): `get_style_state_keys`, `get_state_base_styles` and
//! `get_state_declarations_with_text_clip_escapes`, with block-supports/states.php's `wp_get_state_text_clip_escape`,
//! `wp_get_state_text_fill_reset` and `wp_state_paints_own_background`, which they call.

use speedyspec_hooks::Value;
use speedyspec_wp_theme_json_contracts::schema;

use crate::array_path;
use crate::css::Declaration;
use crate::php_value::{as_str, empty, entries};
use crate::sanitize::viewport;

/// `WP_Style_Engine::VALID_BACKGROUND_CLIP_VALUES`.
const VALID_BACKGROUND_CLIP_VALUES: [&str; 4] =
    ["border-box", "padding-box", "content-box", "text"];

/// `$style['background']['backgroundClip']` when it is one of the values the style engine accepts, else `""`.
fn valid_clip(style: &Value) -> String {
    match array_path::get(style, &["background", "backgroundClip"]) {
        Some(Value::Str(clip)) if VALID_BACKGROUND_CLIP_VALUES.contains(&clip.as_str()) => {
            clip.clone()
        }
        _ => String::new(),
    }
}

fn is_set(style: &Value, path: &[&str]) -> bool {
    array_path::get(style, path).is_some_and(|v| !empty(v))
}

/// `wp_state_paints_own_background( $state_style )`.
fn paints_own_background(style: &Value) -> bool {
    is_set(style, &["background", "gradient"])
        || is_set(style, &["background", "backgroundImage"])
        || is_set(style, &["color", "gradient"])
}

/// `wp_get_state_text_clip_escape( $state_style, $base_clip )`.
fn text_clip_escape(style: &Value, base_clip: &str) -> Option<Declaration> {
    let clip = valid_clip(style);
    if clip.is_empty() || clip == "text" || base_clip != "text" || paints_own_background(style) {
        return None;
    }
    Some(Declaration::text("background-image", "unset"))
}

/// `wp_get_state_text_fill_reset( $state_style )`.
fn text_fill_reset(style: &Value) -> Option<Declaration> {
    if !valid_clip(style).is_empty()
        || !is_set(style, &["color", "text"])
        || paints_own_background(style)
    {
        return None;
    }
    Some(Declaration::text("-webkit-text-fill-color", "currentColor"))
}

fn strings(table: &Value) -> Vec<String> {
    entries(table)
        .iter()
        .flat_map(|(_, list)| entries(list))
        .filter_map(|(_, v)| as_str(v).map(|s| s.into_owned()))
        .collect()
}

/// `get_style_state_keys( $settings )`: the breakpoints, then every block and element pseudo selector and custom
/// state, each once.
fn style_state_keys(settings: &Value) -> Vec<String> {
    let mut keys: Vec<String> =
        viewport::media_queries(array_path::get(settings, &["viewport"]), false)
            .into_iter()
            .map(|(key, _)| key)
            .collect();
    for table in [
        "VALID_BLOCK_PSEUDO_SELECTORS",
        "VALID_ELEMENT_PSEUDO_SELECTORS",
        "VALID_BLOCK_CUSTOM_STATES",
    ] {
        keys.extend(strings(&schema::table(table)));
    }
    let mut unique: Vec<String> = Vec::with_capacity(keys.len());
    for key in keys {
        if !unique.contains(&key) {
            unique.push(key);
        }
    }
    unique
}

/// `get_state_base_styles( $path, $tree, $settings )`: the style objects a state node layers over, nearest first,
/// found by dropping the path's state keys from the innermost outward.
pub(crate) fn state_base_styles(path: &[String], tree: &Value, settings: &Value) -> Vec<Value> {
    let state_keys = style_state_keys(settings);
    let mut base_path: Vec<Option<&String>> = path.iter().map(Some).collect();
    let mut base_styles = Vec::new();
    for index in (0..path.len()).rev() {
        if !state_keys.contains(&path[index]) {
            continue;
        }
        base_path[index] = None;
        let layer_path: Vec<&str> = base_path.iter().flatten().map(|s| s.as_str()).collect();
        if let Some(layer @ Value::Array(_)) = array_path::get(tree, &layer_path) {
            base_styles.push(layer.clone());
        }
    }
    base_styles
}

/// `get_state_declarations_with_text_clip_escapes( $declarations, $state_style, $base_styles )`: when the nearest
/// base style that sets a clip clips to the text, the declarations a state needs to escape it.
pub(crate) fn with_text_clip_escapes(
    mut declarations: Vec<Declaration>,
    state_style: &Value,
    base_styles: &[Value],
) -> Vec<Declaration> {
    let base_clip = base_styles
        .iter()
        .find_map(|base| {
            array_path::get(base, &["background", "backgroundClip"])
                .filter(|clip| !empty(clip))
                .map(|clip| match clip {
                    Value::Str(s) => s.clone(),
                    _ => String::new(),
                })
        })
        .unwrap_or_default();
    if base_clip != "text" {
        return declarations;
    }
    declarations.extend(text_clip_escape(state_style, &base_clip));
    declarations.extend(text_fill_reset(state_style));
    declarations
}
