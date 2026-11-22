//! `WP_Theme_JSON_Schema::migrate`: brings a document of any schema version up to version 3.
//!
//! Chain of Responsibility: each [`MigrationStep`] knows the version it upgrades from and passes its result on to
//! the next step, as WordPress's `switch` falls through. It works on the untyped value, because version 1 and 2
//! documents are not the model's shape.

mod v1_to_v2;
mod v2_to_v3;

use speedyspec_hooks::{Key, Value};
use speedyspec_wp_theme_json_contracts::Origin;
use speedyspec_wp_theme_json_contracts::schema::LATEST_SCHEMA;

use crate::array_path;
use crate::php_loose_eq;

/// One upgrade in the chain.
pub trait MigrationStep {
    /// The schema version this step upgrades from.
    fn from(&self) -> i64;
    /// Upgrades a document of version [`MigrationStep::from`] by one version.
    fn migrate(&self, doc: Value, origin: Origin) -> Value;
}

/// The steps, oldest first.
fn chain() -> [&'static dyn MigrationStep; 2] {
    [&v1_to_v2::V1ToV2, &v2_to_v3::V2ToV3]
}

/// `WP_Theme_JSON_Schema::migrate( $theme_json, $origin )`.
///
/// A document without a `version` (or one that is not an array) is replaced by `['version' => 3]`, as WordPress
/// replaces it. The version is matched with PHP's loose `==`, as WordPress's `switch` matches it; a version no step
/// knows passes through untouched.
pub fn migrate(doc: Value, origin: Origin) -> Value {
    if !array_path::isset(&doc, &["version"]) {
        return Value::Array(vec![(
            Key::Str("version".into()),
            Value::Int(LATEST_SCHEMA),
        )]);
    }
    let version = array_path::get(&doc, &["version"])
        .cloned()
        .unwrap_or(Value::Null);
    let steps = chain();
    let Some(start) = steps
        .iter()
        .position(|s| php_loose_eq(&version, &Value::Int(s.from())))
    else {
        return doc;
    };
    steps[start..]
        .iter()
        .fold(doc, |doc, step| step.migrate(doc, origin))
}
