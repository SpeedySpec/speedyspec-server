//! The rules applied to a layer's presets as they replace the merged ones (Strategy, one per rule).

use speedyspec_hooks::{Key, Value};
use speedyspec_wp_theme_json_contracts::schema::{PresetMetadata, presets_metadata};
use speedyspec_wp_theme_json_contracts::Origin;

use crate::array_path;

/// What a rule may read while one origin's presets replace the merged ones at one settings node.
pub struct PresetContext<'a> {
    /// The merged document so far.
    pub merged: &'a Value,
    /// The settings node's path (`["settings"]` or `["settings", "blocks", name]`).
    pub node: &'a [String],
    /// The node's path to this preset.
    pub base_path: &'a [String],
    /// The preset's metadata.
    pub metadata: &'a PresetMetadata,
    /// The origin whose presets are being placed.
    pub origin: Origin,
    /// Whether default presets win over this origin's presets of the same slug.
    pub prevent_override: bool,
    /// The default presets' slugs at the root settings, by preset path.
    pub global_slugs: &'a Value,
}

/// One rule.
pub trait PresetRule {
    /// Whether the rule applies here.
    fn applies(&self, context: &PresetContext<'_>) -> bool;
    /// The presets after the rule.
    fn apply(&self, context: &PresetContext<'_>, content: Value) -> Value;
}

fn slug(item: &Value) -> Option<&Value> {
    array_path::child(item, &Key::Str("slug".into())).filter(|v| !matches!(v, Value::Null))
}

/// A theme preset without a name takes the name of the default preset with the same slug (`get_name_from_defaults`).
pub struct UseDefaultNames;

impl PresetRule for UseDefaultNames {
    fn applies(&self, context: &PresetContext<'_>) -> bool {
        context.origin == Origin::Theme && context.metadata.use_default_names
    }

    fn apply(&self, context: &PresetContext<'_>, content: Value) -> Value {
        let Value::Array(items) = content else { return content };
        let defaults_path: Vec<&str> =
            context.base_path.iter().map(String::as_str).chain(std::iter::once("default")).collect();
        let defaults = array_path::get(context.merged, &defaults_path).filter(|d| crate::merge::truthy(d));
        Value::Array(
            items
                .into_iter()
                .map(|(key, mut item)| {
                    let named = array_path::isset(&item, &["name"]);
                    if !named
                        && matches!(item, Value::Array(_))
                        && let Some(Value::Array(defaults)) = defaults
                    {
                        let wanted = slug(&item).cloned().unwrap_or(Value::Null);
                        let found = defaults.iter().find(|(_, d)| {
                            array_path::child(d, &Key::Str("slug".into())).is_some_and(|s| *s == wanted)
                        });
                        if let Some((_, default)) = found {
                            let name = array_path::child(default, &Key::Str("name".into())).cloned().unwrap_or(Value::Null);
                            array_path::insert(&mut item, Key::Str("name".into()), name);
                        }
                    }
                    (key, item)
                })
                .collect(),
        )
    }
}

/// When defaults win, theme presets with a default preset's slug are dropped (`get_default_slugs`, `filter_slugs`).
pub struct RemoveDefaultSlugs;

impl PresetRule for RemoveDefaultSlugs {
    fn applies(&self, context: &PresetContext<'_>) -> bool {
        context.origin == Origin::Theme && context.prevent_override
    }

    fn apply(&self, context: &PresetContext<'_>, content: Value) -> Value {
        let path: Vec<&str> = context.metadata.path.iter().map(String::as_str).collect();
        let node_slugs = default_slugs(context.merged, context.node);
        let mut slugs = list(array_path::get(context.global_slugs, &path));
        slugs.extend(list(array_path::get(&node_slugs, &path)));
        if slugs.is_empty() {
            return content;
        }
        let Value::Array(items) = content else { return Value::Array(Vec::new()) };
        Value::Array(
            items
                .into_iter()
                .filter(|(_, item)| slug(item).is_some_and(|s| !slugs.contains(s)))
                .enumerate()
                .map(|(i, (_, item))| (Key::Int(i as i64), item))
                .collect(),
        )
    }
}

fn list(value: Option<&Value>) -> Vec<Value> {
    match value {
        Some(Value::Array(items)) => items.iter().map(|(_, v)| v.clone()).collect(),
        _ => Vec::new(),
    }
}

/// `get_default_slugs( $data, $node_path )`: the default presets' slugs at a node, by preset path.
pub fn default_slugs(data: &Value, node: &[String]) -> Value {
    let mut slugs = Value::Array(Vec::new());
    for metadata in presets_metadata().iter() {
        let path: Vec<&str> = node
            .iter()
            .map(String::as_str)
            .chain(metadata.path.iter().map(String::as_str))
            .chain(std::iter::once("default"))
            .collect();
        let Some(preset) = array_path::get(data, &path).filter(|p| !matches!(p, Value::Null)) else { continue };
        let found: Vec<Value> = match preset {
            Value::Array(items) => items.iter().filter_map(|(_, item)| slug(item).cloned()).collect(),
            _ => Vec::new(),
        };
        let found = Value::Array(found.into_iter().enumerate().map(|(i, v)| (Key::Int(i as i64), v)).collect());
        let target: Vec<&str> = metadata.path.iter().map(String::as_str).collect();
        array_path::set(&mut slugs, &target, found);
    }
    slugs
}
