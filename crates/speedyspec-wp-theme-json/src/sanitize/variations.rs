use speedyspec_hooks::Value;

use super::ValidNames;
use crate::array_path;

/// `unwrap_shared_block_style_variations`: copies `styles.variations` into each block that registers the variation,
/// under the block's own variation data, then removes `styles.variations`.
pub fn unwrap_shared_block_style_variations(doc: Value, valid: &ValidNames) -> Value {
    let shared = array_path::get(&doc, &["styles", "variations"]).cloned();
    if !php_non_empty(shared.as_ref()) || valid.variations.is_empty() {
        return doc;
    }
    let shared = shared.unwrap_or(Value::Null);
    let mut new = doc;
    for (block, names) in &valid.variations {
        for name in names {
            let block_level = array_path::get(&new, &["styles", "blocks", block, "variations", name])
                .filter(|v| !matches!(v, Value::Null))
                .cloned()
                .unwrap_or(Value::Array(Vec::new()));
            let top_level = array_path::get(&shared, &[name])
                .filter(|v| !matches!(v, Value::Null))
                .cloned()
                .unwrap_or(Value::Array(Vec::new()));
            let merged = array_path::replace_recursive(top_level, block_level);
            if php_non_empty(Some(&merged)) {
                array_path::set(&mut new, &["styles", "blocks", block, "variations", name], merged);
            }
        }
    }
    array_path::unset(&mut new, &["styles", "variations"]);
    new
}

/// `! empty( $value )`.
fn php_non_empty(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) | Some(Value::Bool(false)) | Some(Value::Int(0)) => false,
        Some(Value::Float(f)) => *f != 0.0,
        Some(Value::Str(s)) => !(s.is_empty() || s == "0"),
        Some(Value::Bytes(b)) => !(b.is_empty() || b == b"0"),
        Some(Value::Array(a)) => !a.is_empty(),
        Some(_) => true,
    }
}
