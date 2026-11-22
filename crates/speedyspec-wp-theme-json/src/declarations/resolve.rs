//! `WP_Theme_JSON::resolve_variables( $theme_json )`: preset and custom CSS variables in styles replaced by their
//! values (`convert_variables_to_value`).

use std::sync::LazyLock;

use fancy_regex::Regex;
use speedyspec_hooks::{Key, Value};
use speedyspec_wp_theme_json_contracts::Origin;

use crate::php::to_php_string;
use crate::php_value::empty;
use crate::presets::FluidFontSize;
use crate::presets::vars::{compute_preset_vars, compute_theme_vars};

/// `/var\(([^),]+)?,?\s?(\S+)?\)/` with PCRE's ASCII `\s` and `\S`.
static VAR: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"var\(([^),]+)?,?[ \t\n\x{0b}\x{0c}\r]?([^ \t\n\x{0b}\x{0c}\r]+)?\)")
        .expect("compiles")
});

/// `str_replace( $search, $replace, $subject )` with arrays: each pair in turn over the result of the last, empty
/// search strings skipped.
fn str_replace_pairs(subject: Vec<u8>, pairs: &[(&[u8], Vec<u8>)]) -> Vec<u8> {
    let mut out = subject;
    for (search, replace) in pairs {
        if search.is_empty() {
            continue;
        }
        let mut next = Vec::with_capacity(out.len());
        let mut i = 0;
        while i < out.len() {
            if out[i..].starts_with(search) {
                next.extend_from_slice(replace);
                i += search.len();
            } else {
                next.push(out[i]);
                i += 1;
            }
        }
        out = next;
    }
    out
}

fn lookup<'v>(values: &'v [(String, Value)], key: &str) -> Option<&'v Value> {
    values
        .iter()
        .rev()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v)
        .filter(|v| !matches!(v, Value::Null))
}

/// `convert_variables_to_value( $styles, $values )`.
pub fn convert_variables_to_value(styles: Value, values: &[(String, Value)]) -> Value {
    let Value::Array(entries) = styles else {
        return styles;
    };
    let mut out: Vec<(Key, Value)> = Vec::with_capacity(entries.len());
    for (key, style) in entries {
        if empty(&style) {
            out.push((key, style));
            continue;
        }
        if let Value::Array(_) = style {
            out.push((key, convert_variables_to_value(style, values)));
            continue;
        }
        let text = String::from_utf8_lossy(&to_php_string(&style)).into_owned();
        let matches: Vec<(String, String, String)> = VAR
            .captures_iter(&text)
            .filter_map(Result::ok)
            .map(|c| {
                let group = |i: usize| c.get(i).map_or(String::new(), |m| m.as_str().to_owned());
                (group(0), group(1), group(2))
            })
            .collect();
        if matches.is_empty() {
            out.push((key, style));
            continue;
        }
        let mut resolved = to_php_string(&style);
        for (rule, var_part, fallback) in &matches {
            let key_in_values = format!("var({var_part})");
            let rule_value =
                lookup(values, &key_in_values).map_or(rule.as_bytes().to_vec(), to_php_string);
            let fallback_value =
                lookup(values, fallback).map_or(fallback.as_bytes().to_vec(), to_php_string);
            resolved = str_replace_pairs(
                resolved,
                &[
                    (rule.as_bytes(), rule_value),
                    (fallback.as_bytes(), fallback_value),
                ],
            );
        }
        out.push((key, crate::codec::string_value(&resolved)));
    }
    Value::Array(out)
}

/// The variables `resolve_variables` replaces: `var(<name>)` → value, preset variables then custom ones.
pub fn variable_values(settings: &Value, fluid: &FluidFontSize) -> Vec<(String, Value)> {
    compute_preset_vars(settings, &Origin::ALL, fluid)
        .into_iter()
        .chain(compute_theme_vars(settings))
        .map(|d| (format!("var({})", d.name), d.value))
        .collect()
}
