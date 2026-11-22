//! Feature-level declarations (`get_feature_declarations_for_node`) and the adjustments WordPress makes to them
//! and to separators.

use speedyspec_hooks::{Key, Value};

use super::{DeclarationContext, compute_style_properties};
use crate::array_path;
use crate::css::Declaration;
use crate::php::{float_prefix, float_to_string};
use crate::php_value::{as_str, empty, entries, key, key_str};

/// Declarations by feature selector, in the order WordPress adds the selectors.
pub type FeatureDeclarations = Vec<(String, Vec<Declaration>)>;

fn add(declarations: &mut FeatureDeclarations, selector: String, new: Vec<Declaration>) {
    match declarations.iter_mut().find(|(s, _)| *s == selector) {
        Some(slot) => slot.1.extend(new),
        None => declarations.push((selector, new)),
    }
}

fn wrap(feature: &str, inner: Value) -> Value {
    Value::Array(vec![(key(feature), inner)])
}

/// `get_feature_declarations_for_node( $metadata, &$node )`: the declarations for the node's features that have
/// their own selectors, removing those features from `node` as WordPress does (so the node's own rule leaves them
/// out).
pub fn feature_declarations_for_node(
    metadata: &Value,
    node: &mut Value,
    settings: &Value,
    theme_json: &Value,
    ctx: DeclarationContext<'_>,
) -> FeatureDeclarations {
    let mut declarations = FeatureDeclarations::new();
    let Some(selectors) =
        array_path::get(metadata, &["selectors"]).filter(|v| !matches!(v, Value::Null))
    else {
        return declarations;
    };
    for (feature, feature_selectors) in entries(selectors) {
        let feature = key_str(feature);
        let f = feature.as_str();
        if f == "root" || f == "css" || array_path::get(node, &[f]).is_none_or(empty) {
            continue;
        }
        if let Value::Array(subs) = feature_selectors {
            for (subfeature, sub_selector) in subs {
                let subfeature = key_str(subfeature);
                let sf = subfeature.as_str();
                if sf == "root" {
                    continue;
                }
                let Some(sub_value) = array_path::get(node, &[f, sf])
                    .filter(|v| !empty(v))
                    .cloned()
                else {
                    continue;
                };
                let sub_node = wrap(f, wrap(sf, sub_value));
                let new = compute_style_properties(
                    &sub_node,
                    settings,
                    None,
                    Some(theme_json),
                    None,
                    false,
                    ctx,
                );
                add(
                    &mut declarations,
                    String::from_utf8_lossy(&crate::php::to_php_string(sub_selector)).into_owned(),
                    new,
                );
                array_path::unset(node, &[f, sf]);
            }
        }
        let root_selector = match feature_selectors {
            Value::Str(s) => Some(s.clone()),
            other => array_path::get(other, &["root"])
                .filter(|r| !empty(r))
                .map(|r| String::from_utf8_lossy(&crate::php::to_php_string(r)).into_owned()),
        };
        if let Some(selector) = root_selector {
            let feature_value = array_path::get(node, &[f]).cloned().unwrap_or(Value::Null);
            let new = compute_style_properties(
                &wrap(f, feature_value),
                settings,
                None,
                Some(theme_json),
                None,
                false,
                ctx,
            );
            add(&mut declarations, selector, new);
            array_path::unset(node, &[f]);
        }
    }
    declarations
}

/// `update_separator_declarations( $declarations )`: a separator with only a background colour also gets it as
/// its text colour.
pub fn update_separator_declarations(mut declarations: Vec<Declaration>) -> Vec<Declaration> {
    let mut background = Value::Null;
    let mut border = false;
    let mut text = false;
    for d in &declarations {
        if d.name == "background-color" && empty(&background) && !matches!(d.value, Value::Null) {
            background = d.value.clone();
        } else if d.name == "border-color" {
            border = true;
        } else if d.name == "color" {
            text = true;
        }
        if !empty(&background) && border && text {
            break;
        }
    }
    if !empty(&background) && !border && !text {
        declarations.push(Declaration::new("color", background));
    }
    declarations
}

/// `update_paragraph_text_indent_selector( $feature_declarations, $settings, $block_name )`.
pub fn update_paragraph_text_indent_selector(
    mut declarations: FeatureDeclarations,
    settings: &Value,
    block_name: Option<&str>,
) -> FeatureDeclarations {
    if block_name != Some("core/paragraph") {
        return declarations;
    }
    let setting = array_path::get(
        settings,
        &["blocks", "core/paragraph", "typography", "textIndent"],
    )
    .filter(|v| !matches!(v, Value::Null))
    .or_else(|| {
        array_path::get(settings, &["typography", "textIndent"])
            .filter(|v| !matches!(v, Value::Null))
    });
    if !matches!(setting, Some(Value::Str(s)) if s == "all") {
        return declarations;
    }
    let old = ".wp-block-paragraph + .wp-block-paragraph";
    let new = ".wp-block-paragraph";
    if let Some(index) = declarations.iter().position(|(s, _)| s == old) {
        let (_, moved) = declarations.remove(index);
        match declarations.iter_mut().find(|(s, _)| s == new) {
            Some(slot) => slot.1 = moved,
            None => declarations.push((new.to_owned(), moved)),
        }
    }
    declarations
}

/// `update_button_width_declarations( $feature_declarations, $settings )`: a percentage button width (direct or
/// through a dimension preset) becomes a `calc()` that leaves room for the block gap.
pub fn update_button_width_declarations(
    mut declarations: FeatureDeclarations,
    settings: &Value,
) -> FeatureDeclarations {
    let Some((_, button)) = declarations
        .iter_mut()
        .find(|(s, _)| s == ".wp-block-button")
    else {
        return declarations;
    };
    for declaration in button.iter_mut() {
        if declaration.name != "width" || matches!(declaration.value, Value::Null) {
            continue;
        }
        let Some(value) = as_str(&declaration.value).map(|c| c.into_owned()) else {
            continue;
        };
        let mut percentage = None;
        if value.ends_with('%') {
            percentage = Some(float_prefix(&value));
        }
        let prefix = "var(--wp--preset--dimension--";
        if percentage.is_none() && value.starts_with(prefix) {
            let slug = if value.len() > prefix.len() {
                &value[prefix.len()..value.len() - 1]
            } else {
                ""
            };
            let mut sizes: Vec<(Key, Value)> = entries(
                array_path::get(
                    settings,
                    &["blocks", "core/button", "dimensions", "dimensionSizes"],
                )
                .unwrap_or(&Value::Null),
            )
            .to_vec();
            for (k, v) in entries(
                array_path::get(settings, &["dimensions", "dimensionSizes"])
                    .unwrap_or(&Value::Null),
            ) {
                if !sizes.iter().any(|(sk, _)| sk == k) {
                    sizes.push((k.clone(), v.clone()));
                }
            }
            'search: for (_, origin_sizes) in &sizes {
                let Value::Array(presets) = origin_sizes else {
                    continue;
                };
                for (_, preset) in presets {
                    let slug_matches = matches!(array_path::get(preset, &["slug"]), Some(Value::Str(s)) if s == slug);
                    if slug_matches && array_path::isset(preset, &["size"]) {
                        if let Some(Value::Str(size)) = array_path::get(preset, &["size"])
                            && size.ends_with('%')
                        {
                            percentage = Some(float_prefix(size));
                        }
                        break 'search;
                    }
                }
            }
        }
        let Some(p) = percentage else { continue };
        let p = float_to_string(p);
        declaration.value = Value::Str(format!(
            "calc({p} * 1% - (var(--wp--style--block-gap, 0.5em) * (1 - {p} / 100)))"
        ));
    }
    declarations
}
