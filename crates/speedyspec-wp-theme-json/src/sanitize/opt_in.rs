use speedyspec_hooks::{Key, Value};
use speedyspec_wp_theme_json_contracts::schema;

use crate::array_path;

/// `maybe_opt_in_into_settings`: `appearanceTools: true` (strictly) turns on each setting in
/// `APPEARANCE_TOOLS_OPT_INS` that is not set, at the root and in each block, and is then removed.
pub fn opt_in_into_settings(mut doc: Value) -> Value {
    if matches!(array_path::get(&doc, &["settings", "appearanceTools"]), Some(Value::Bool(true)))
        && let Some(settings) = child_mut(&mut doc, "settings")
    {
        do_opt_in(settings);
    }
    if let Some(Value::Array(blocks)) = child_mut(&mut doc, "settings").and_then(|s| child_mut(s, "blocks")) {
        for (_, block) in blocks.iter_mut() {
            if matches!(array_path::get(block, &["appearanceTools"]), Some(Value::Bool(true))) {
                do_opt_in(block);
            }
        }
    }
    doc
}

fn child_mut<'a>(value: &'a mut Value, name: &str) -> Option<&'a mut Value> {
    match value {
        Value::Array(entries) => entries.iter_mut().find(|(k, _)| *k == Key::Str(name.into())).map(|(_, v)| v),
        _ => None,
    }
}

fn do_opt_in(context: &mut Value) {
    if let Some(Value::Array(paths)) = schema::constant("APPEARANCE_TOOLS_OPT_INS") {
        for (_, path) in &paths {
            let Value::Array(parts) = path else { continue };
            let parts: Vec<&str> = parts.iter().filter_map(|(_, p)| if let Value::Str(s) = p { Some(s.as_str()) } else { None }).collect();
            if array_path::get(context, &parts).is_none() {
                array_path::set(context, &parts, Value::Bool(true));
            }
        }
    }
    array_path::unset(context, &["appearanceTools"]);
}
