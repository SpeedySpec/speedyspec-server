//! `wp_get_block_css_selector( $block_type, $target, $fallback )`.

use speedyspec_hooks::Value;
use speedyspec_wp_theme_json_contracts::ports::RegisteredBlock;

use super::scope_selector;
use crate::array_path;
use crate::php::to_php_string;
use crate::php_value::{as_str, empty};

fn root_selector(block: &RegisteredBlock, has_selectors: bool) -> String {
    if has_selectors
        && let Some(root) =
            array_path::get(&block.selectors, &["root"]).filter(|v| !matches!(v, Value::Null))
    {
        return String::from_utf8_lossy(&to_php_string(root)).into_owned();
    }
    if let Some(experimental) =
        array_path::get(&block.supports, &["__experimentalSelector"]).and_then(as_str)
    {
        return experimental.into_owned();
    }
    let name = block
        .name
        .replacen("core/", "", usize::MAX)
        .replace('/', "-");
    format!(".wp-block-{name}")
}

fn string_of(value: Option<&Value>) -> Option<String> {
    value
        .filter(|v| !matches!(v, Value::Null))
        .map(|v| String::from_utf8_lossy(&to_php_string(v)).into_owned())
}

/// `wp_get_block_css_selector()`: the block's root selector (`target` `"root"`), or a feature's (`"color"`) or
/// subfeature's (`"color.text"`, or a list of steps) selector, falling back to the root or feature selector
/// when `fallback` is set. `None` is WordPress's `null`.
pub fn block_css_selector(
    block: &RegisteredBlock,
    target: &Value,
    fallback: bool,
) -> Option<String> {
    if empty(target) {
        return None;
    }
    let has_selectors = !empty(&block.selectors);
    let root = root_selector(block, has_selectors);
    if matches!(target, Value::Str(t) if t == "root") {
        return Some(root);
    }
    let steps: Vec<String> = match target {
        Value::Array(items) => items
            .iter()
            .map(|(_, v)| String::from_utf8_lossy(&to_php_string(v)).into_owned())
            .collect(),
        other => String::from_utf8_lossy(&to_php_string(other))
            .split('.')
            .map(str::to_owned)
            .collect(),
    };
    if steps.len() == 1 {
        let fallback_selector = fallback.then(|| root.clone());
        let feature = steps[0].as_str();
        if has_selectors {
            if let Some(found) =
                array_path::get(&block.selectors, &[feature, "root"]).filter(|v| !empty(v))
            {
                return string_of(Some(found));
            }
            return match array_path::get(&block.selectors, &[feature]).and_then(as_str) {
                Some(s) => Some(s.into_owned()),
                None => fallback_selector,
            };
        }
        return match array_path::get(&block.supports, &[feature, "__experimentalSelector"])
            .filter(|v| !matches!(v, Value::Null))
        {
            None => fallback_selector,
            Some(found) => Some(scope_selector(
                &root,
                &String::from_utf8_lossy(&to_php_string(found)),
            )),
        };
    }
    if has_selectors {
        let path: Vec<&str> = steps.iter().map(String::as_str).collect();
        if let Some(found) = array_path::get(&block.selectors, &path).filter(|v| !empty(v)) {
            return string_of(Some(found));
        }
    }
    if fallback {
        return block_css_selector(block, &Value::Str(steps[0].clone()), fallback);
    }
    None
}
