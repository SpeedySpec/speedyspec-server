//! Style variations: the `styles/` files of the theme and its parent, and injecting block style variations.

use speedyspec_hooks::Value;

use crate::array_path::{self, replace_recursive};
use crate::php::to_php_string;
use crate::php_value::{empty, entries, key_str};
use crate::presets::to_kebab_case;

/// `style_variation_has_scope( $variation, $scope )`.
pub fn has_scope(variation: &Value, scope: &str) -> bool {
    match scope {
        "block" => array_path::isset(variation, &["blockTypes"]),
        "theme" => !array_path::isset(variation, &["blockTypes"]),
        _ => false,
    }
}

/// `recursively_iterate_json( $dir )`: every `.json` file below `dir`, as `dir/…/name.json`.
fn json_files(dir: &str, out: &mut Vec<String>) {
    let Ok(read) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in read.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let path = format!("{dir}/{name}");
        // `RecursiveDirectoryIterator` without `FOLLOW_SYMLINKS`: a symlinked directory is a leaf, not descended.
        let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
        if is_dir {
            json_files(&path, out);
        } else {
            let body = path.strip_suffix('\n').unwrap_or(&path);
            if body.len() > 5 && body.to_ascii_lowercase().ends_with(".json") {
                out.push(path);
            }
        }
    }
}

fn basename(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// The variation files `get_style_variations` reads: the child's, then the parent's not shadowed by a child file
/// of the same name, sorted by path.
pub fn variation_files(stylesheet_dir: &str, template_dir: &str) -> Vec<String> {
    let base = format!("{stylesheet_dir}/styles");
    let template = format!("{template_dir}/styles");
    let mut files = Vec::new();
    if std::path::Path::new(&base).is_dir() {
        json_files(&base, &mut files);
    }
    if std::path::Path::new(&template).is_dir() && template != base {
        let mut parent = Vec::new();
        json_files(&template, &mut parent);
        parent.retain(|p| !files.iter().any(|c| basename(c) == basename(p)));
        files.extend(parent);
    }
    files.sort();
    files.dedup();
    files
}

/// `inject_variations_from_block_style_variation_files( $data, $variations )`.
pub fn inject_from_files(mut data: Value, variations: &Value) -> Value {
    if empty(variations) {
        return data;
    }
    for (_, variation) in entries(variations) {
        let (Some(styles), Some(block_types)) = (
            array_path::get(variation, &["styles"]).filter(|v| !empty(v)),
            array_path::get(variation, &["blockTypes"]).filter(|v| !empty(v)),
        ) else {
            continue;
        };
        let name = match array_path::get(variation, &["slug"]).filter(|v| !matches!(v, Value::Null))
        {
            Some(slug) => String::from_utf8_lossy(&to_php_string(slug)).into_owned(),
            None => to_kebab_case(&to_php_string(
                array_path::get(variation, &["title"]).unwrap_or(&Value::Null),
            )),
        };
        let mut styles = styles.clone();
        for (_, block_type) in entries(block_types) {
            let block_type = String::from_utf8_lossy(&to_php_string(block_type)).into_owned();
            styles = merge_overrides(styles, &data, &block_type, &name);
            array_path::set(
                &mut data,
                &["styles", "blocks", &block_type, "variations", &name],
                styles.clone(),
            );
        }
    }
    data
}

/// The theme's own top-level and block-level variation styles, over the variation's.
fn merge_overrides(mut styles: Value, data: &Value, block_type: &str, name: &str) -> Value {
    if let Some(top) = array_path::get(data, &["styles", "variations", name]).filter(|v| !empty(v))
    {
        styles = replace_recursive(styles, top.clone());
    }
    if let Some(block) =
        array_path::get(data, &["styles", "blocks", block_type, "variations", name])
            .filter(|v| !empty(v))
    {
        styles = replace_recursive(styles, block.clone());
    }
    styles
}

/// `inject_variations_from_block_styles_registry( $data )`.
pub fn inject_from_registry(mut data: Value, registered: &Value) -> Value {
    for (block_type, variations) in entries(registered) {
        let block_type = key_str(block_type);
        for (name, variation) in entries(variations) {
            let name = key_str(name);
            let Some(style_data) =
                array_path::get(variation, &["style_data"]).filter(|v| !empty(v))
            else {
                continue;
            };
            let style_data = merge_overrides(style_data.clone(), &data, &block_type, &name);
            array_path::set(
                &mut data,
                &["styles", "blocks", &block_type, "variations", &name],
                style_data,
            );
        }
    }
    data
}
