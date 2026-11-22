//! `remove_insecure_styles`, `remove_insecure_element_styles`, `remove_insecure_inner_block_styles` and
//! `remove_indirect_properties`.

use speedyspec_hooks::{Key, Value};
use speedyspec_wp_theme_json_contracts::schema;

use super::{Security, pseudo_list, put};
use crate::array_path;
use crate::declarations::compute_style_properties;
use crate::php::to_php_string;
use crate::php_value::{empty, entries, key_str};

fn steps(path: &Value) -> Vec<String> {
    entries(path)
        .iter()
        .map(|(_, v)| String::from_utf8_lossy(&to_php_string(v)).into_owned())
        .collect()
}

impl Security<'_> {
    /// `remove_insecure_styles( $input )`: only the properties whose declarations kses keeps, plus the safe
    /// indirect properties.
    pub(crate) fn remove_insecure_styles(&self, input: &Value) -> Value {
        let mut output = Value::Array(Vec::new());
        let properties = schema::table("PROPERTIES_METADATA");
        for declaration in compute_style_properties(
            input,
            &Value::Array(Vec::new()),
            None,
            None,
            None,
            false,
            self.decl(),
        ) {
            if !self.safe(&declaration.name, &declaration.value) {
                continue;
            }
            let Some(path) = array_path::get(&properties, &[&declaration.name]) else {
                continue;
            };
            let path = steps(path);
            let refs: Vec<&str> = path.iter().map(String::as_str).collect();
            match array_path::get(input, &refs) {
                Some(Value::Array(_)) | None => {}
                Some(value) => array_path::set(&mut output, &refs, value.clone()),
            }
        }
        self.remove_indirect_properties(input, &mut output);
        output
    }

    /// `remove_indirect_properties( $input, &$output )`.
    pub(crate) fn remove_indirect_properties(&self, input: &Value, output: &mut Value) {
        let indirect = schema::table("INDIRECT_PROPERTIES_METADATA");
        for (property, paths) in entries(&indirect) {
            let property = key_str(property);
            for (_, path) in entries(paths) {
                let path = steps(path);
                let refs: Vec<&str> = path.iter().map(String::as_str).collect();
                if let Some(value @ (Value::Str(_) | Value::Bytes(_))) =
                    array_path::get(input, &refs)
                    && self.safe(&property, value)
                {
                    array_path::set(output, &refs, value.clone());
                }
            }
        }
    }

    /// `remove_insecure_element_styles( $elements, $responsive_media_queries )`.
    pub(crate) fn remove_insecure_element_styles(
        &self,
        elements: &Value,
        breakpoints: Option<&[(String, String)]>,
    ) -> Value {
        let mut sanitized = Value::Array(Vec::new());
        let element_names = schema::table("ELEMENTS");
        for (name, _) in entries(&element_names) {
            let name = key_str(name);
            let Some(element_input) = array_path::get(elements, &[&name]).filter(|v| !empty(v))
            else {
                continue;
            };
            let mut element_output = self.remove_insecure_styles(element_input);
            let pseudos = pseudo_list("VALID_ELEMENT_PSEUDO_SELECTORS", &name);
            for pseudo in pseudos.iter().flatten() {
                if let Some(pseudo_input) =
                    array_path::get(element_input, &[pseudo]).filter(|v| !matches!(v, Value::Null))
                {
                    put(
                        &mut element_output,
                        pseudo,
                        self.remove_insecure_styles(pseudo_input),
                    );
                }
            }
            for (breakpoint, _) in breakpoints.unwrap_or(&[]) {
                let Some(bp_input) = array_path::get(element_input, &[breakpoint])
                    .filter(|v| !matches!(v, Value::Null))
                else {
                    continue;
                };
                let mut bp_output = self.remove_insecure_styles(bp_input);
                for pseudo in pseudos.iter().flatten() {
                    if let Some(pseudo_input) =
                        array_path::get(bp_input, &[pseudo]).filter(|v| !matches!(v, Value::Null))
                    {
                        put(
                            &mut bp_output,
                            pseudo,
                            self.remove_insecure_styles(pseudo_input),
                        );
                    }
                }
                put(&mut element_output, breakpoint, bp_output);
            }
            put(&mut sanitized, &name, element_output);
        }
        sanitized
    }

    /// `remove_insecure_inner_block_styles( $blocks, $responsive_media_queries )`.
    pub(crate) fn remove_insecure_inner_block_styles(
        &self,
        blocks: &Value,
        breakpoints: Option<&[(String, String)]>,
    ) -> Value {
        let mut sanitized: Vec<(Key, Value)> = Vec::new();
        for (block_type, block_input) in entries(blocks) {
            let mut block_output = self.remove_insecure_styles(block_input);
            if let Some(elements) =
                array_path::get(block_input, &["elements"]).filter(|v| !matches!(v, Value::Null))
            {
                put(
                    &mut block_output,
                    "elements",
                    self.remove_insecure_element_styles(elements, breakpoints),
                );
            }
            let name = key_str(block_type);
            for (breakpoint, _) in breakpoints.unwrap_or(&[]) {
                let Some(bp_input) = array_path::get(block_input, &[breakpoint])
                    .filter(|v| !matches!(v, Value::Null))
                else {
                    continue;
                };
                let mut bp_output = self.remove_insecure_styles(bp_input);
                for pseudo in pseudo_list("VALID_BLOCK_PSEUDO_SELECTORS", &name)
                    .iter()
                    .flatten()
                {
                    if let Some(pseudo_input) =
                        array_path::get(bp_input, &[pseudo]).filter(|v| !matches!(v, Value::Null))
                    {
                        put(
                            &mut bp_output,
                            pseudo,
                            self.remove_insecure_styles(pseudo_input),
                        );
                    }
                }
                put(&mut block_output, breakpoint, bp_output);
            }
            match sanitized.iter_mut().find(|(k, _)| k == block_type) {
                Some(slot) => slot.1 = block_output,
                None => sanitized.push((block_type.clone(), block_output)),
            }
        }
        Value::Array(sanitized)
    }
}
