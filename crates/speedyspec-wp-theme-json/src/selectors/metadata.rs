//! The computed block metadata (`WP_Theme_JSON::get_blocks_metadata()`): each block's root selector, feature
//! selectors, element selectors, duotone selector, style variation selectors and custom states, in WordPress's
//! array form, recomputed only when the registries change.

use std::cell::RefCell;

use speedyspec_hooks::{Key, Value};
use speedyspec_wp_theme_json_contracts::ports::{BlockMetadata, RegisteredBlock};
use speedyspec_wp_theme_json_contracts::schema;

use super::{
    block_css_selector, block_style_variation_selector, prepend_to_selector, scope_selector,
};
use crate::array_path;
use crate::php::to_php_string;
use crate::php_value::{as_str, empty, entries, key_str, map, s};

/// `get_block_element_selectors( $root_selector )`: each element's selector inside the block.
pub fn element_selectors(root_selector: &str) -> Value {
    let block_selectors: Vec<&str> = root_selector.split(',').collect();
    let elements = schema::table("ELEMENTS");
    let mut out = Vec::new();
    for (name, el_selector) in entries(&elements) {
        let el_selector = as_str(el_selector)
            .map(|c| c.into_owned())
            .unwrap_or_default();
        let mut parts: Vec<String> = Vec::new();
        for selector in &block_selectors {
            if *selector == el_selector {
                parts = vec![el_selector.clone()];
                break;
            }
            parts.push(prepend_to_selector(&el_selector, &format!("{selector} ")));
        }
        out.push((name.clone(), s(parts.join(","))));
    }
    Value::Array(out)
}

/// `get_block_selectors( $block_type, $root_selector )`.
fn block_selectors(block: &RegisteredBlock, root: &str) -> Value {
    if !empty(&block.selectors) {
        return block.selectors.clone();
    }
    let mut selectors = vec![(Key::Str("root".into()), s(root))];
    let features = schema::table("BLOCK_SUPPORT_FEATURE_LEVEL_SELECTORS");
    for (key, feature) in entries(&features) {
        if let Some(found) = block_css_selector(block, &s(key_str(key)), false) {
            let feature =
                crate::array_path::php_key(&String::from_utf8_lossy(&to_php_string(feature)));
            selectors.push((feature, map(vec![("root", s(found))])));
        }
    }
    Value::Array(selectors)
}

/// One block's metadata entry.
fn block_entry(block: &RegisteredBlock) -> Value {
    let root = block_css_selector(block, &s("root"), false).unwrap_or_default();
    let mut entry = vec![
        (Key::Str("selector".into()), s(root.clone())),
        (Key::Str("selectors".into()), block_selectors(block, &root)),
    ];
    let elements = element_selectors(&root);
    if !empty(&elements) {
        entry.push((Key::Str("elements".into()), elements));
    }
    let mut duotone = block_css_selector(block, &s("filter.duotone"), false);
    if duotone.is_none()
        && let Some(support) = array_path::get(&block.supports, &["color", "__experimentalDuotone"])
            .filter(|v| !empty(v))
    {
        duotone = Some(scope_selector(
            &root,
            &String::from_utf8_lossy(&to_php_string(support)),
        ));
    }
    if let Some(duotone) = duotone {
        entry.push((Key::Str("duotone".into()), s(duotone)));
    }
    if !block.style_names.is_empty() {
        let variations = block
            .style_names
            .iter()
            .map(|name| {
                (
                    crate::array_path::php_key(name),
                    s(block_style_variation_selector(name, &root)),
                )
            })
            .collect();
        entry.push((Key::Str("styleVariations".into()), Value::Array(variations)));
    }
    if let Some(states @ Value::Array(_)) =
        array_path::get(&block.selectors, &["states"]).filter(|v| !empty(v))
    {
        entry.push((Key::Str("states".into()), states.clone()));
    }
    Value::Array(entry)
}

/// `get_blocks_metadata()` for these blocks: block name → metadata, in registration order.
pub fn compute_blocks_metadata(blocks: &[RegisteredBlock]) -> Value {
    Value::Array(
        blocks
            .iter()
            .map(|b| (crate::array_path::php_key(&b.name), block_entry(b)))
            .collect(),
    )
}

/// Holds the last computed metadata and the change marker it was computed for.
#[derive(Default)]
pub struct BlocksMetadataCache {
    last: RefCell<Option<(u64, std::rc::Rc<Value>)>>,
}

impl BlocksMetadataCache {
    /// The metadata for the blocks registered now, recomputed when the registries changed.
    pub fn get(&self, blocks: &dyn BlockMetadata) -> std::rc::Rc<Value> {
        let marker = blocks.change_marker();
        if let Some((seen, value)) = &*self.last.borrow()
            && *seen == marker
        {
            return value.clone();
        }
        let value = std::rc::Rc::new(compute_blocks_metadata(&blocks.registered()));
        *self.last.borrow_mut() = Some((marker, value.clone()));
        value
    }
}
