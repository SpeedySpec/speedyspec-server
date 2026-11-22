use speedyspec_hooks::{Key, Value};
use speedyspec_wp_theme_json_contracts::Origin;

use super::MigrationStep;
use crate::array_path;

/// `WP_Theme_JSON_Schema::V1_TO_V2_RENAMED_PATHS`.
const RENAMED_PATHS: [(&str, &str); 4] = [
    ("border.customRadius", "border.radius"),
    ("spacing.customMargin", "spacing.margin"),
    ("spacing.customPadding", "spacing.padding"),
    ("typography.customLineHeight", "typography.lineHeight"),
];

/// `migrate_v1_to_v2`: renames four settings, at the root and in every block.
pub struct V1ToV2;

impl MigrationStep for V1ToV2 {
    fn from(&self) -> i64 {
        1
    }

    fn migrate(&self, mut doc: Value, _origin: Origin) -> Value {
        if array_path::isset(&doc, &["settings"]) {
            let settings = array_path::get(&doc, &["settings"]).cloned().unwrap_or(Value::Null);
            array_path::insert(&mut doc, Key::Str("settings".into()), rename_paths(settings));
        }
        array_path::insert(&mut doc, Key::Str("version".into()), Value::Int(2));
        doc
    }
}

/// `rename_paths`: the root settings, then each block's.
fn rename_paths(mut settings: Value) -> Value {
    rename_settings(&mut settings);
    if let Some(Value::Array(_)) = array_path::get(&settings, &["blocks"])
        && let Value::Array(entries) = &mut settings
        && let Some((_, Value::Array(blocks))) = entries.iter_mut().find(|(k, _)| *k == Key::Str("blocks".into()))
    {
        for (_, block) in blocks.iter_mut() {
            rename_settings(block);
        }
    }
    settings
}

/// `rename_settings`: moves each non-null old path's value to its new path.
fn rename_settings(settings: &mut Value) {
    for (original, renamed) in RENAMED_PATHS {
        let original: Vec<&str> = original.split('.').collect();
        let renamed: Vec<&str> = renamed.split('.').collect();
        match array_path::get(settings, &original) {
            Some(Value::Null) | None => {}
            Some(value) => {
                let value = value.clone();
                array_path::set(settings, &renamed, value);
                array_path::unset(settings, &original);
            }
        }
    }
}
