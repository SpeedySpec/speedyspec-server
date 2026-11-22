use speedyspec_hooks::{Key, Value};
use speedyspec_wp_theme_json_contracts::Origin;

use super::MigrationStep;
use crate::array_path;

/// `migrate_v2_to_v3`: a theme that defines its own font or spacing sizes no longer gets the defaults.
///
/// User data (`custom`) only has its version bumped.
pub struct V2ToV3;

impl MigrationStep for V2ToV3 {
    fn from(&self) -> i64 {
        2
    }

    fn migrate(&self, old: Value, origin: Origin) -> Value {
        let mut new = old.clone();
        array_path::insert(&mut new, Key::Str("version".into()), Value::Int(3));
        if origin == Origin::Custom {
            return new;
        }
        if array_path::isset(&old, &["settings", "typography", "fontSizes"]) {
            array_path::set(
                &mut new,
                &["settings", "typography", "defaultFontSizes"],
                Value::Bool(false),
            );
        }
        let sizes = array_path::isset(&old, &["settings", "spacing", "spacingSizes"]);
        if sizes || array_path::isset(&old, &["settings", "spacing", "spacingScale"]) {
            array_path::set(
                &mut new,
                &["settings", "spacing", "defaultSpacingSizes"],
                Value::Bool(false),
            );
        }
        if sizes {
            array_path::unset(&mut new, &["settings", "spacing", "spacingScale"]);
        }
        new
    }
}
