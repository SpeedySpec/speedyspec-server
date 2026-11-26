use speedyspec_hooks::Value;

use crate::codec::string_value;
use crate::presets::to_kebab_case;

/// `convert_custom_properties`: `var:preset|color|red` becomes `var(--wp--preset--color--red)`. A preset reference's
/// slug is kebab-cased (WordPress 7.2), so it names the custom property generated from the preset: `n27` becomes
/// `n-27`.
pub fn convert_custom_properties(value: &[u8]) -> Vec<u8> {
    let Some(rest) = value.strip_prefix(b"var:") else {
        return value.to_vec();
    };
    let mut parts: Vec<Vec<u8>> = rest.split(|&c| c == b'|').map(<[u8]>::to_vec).collect();
    if parts.len() == 3 && parts[0] == b"preset" {
        parts[2] = to_kebab_case(&parts[2]).into_bytes();
    }
    let mut out = b"var(--wp--".to_vec();
    out.extend_from_slice(&parts.join(&b"--"[..]));
    out.push(b')');
    out
}

/// `resolve_custom_css_format`: converts every string starting with `var:` anywhere in the tree.
pub fn resolve_custom_css_format(tree: Value) -> Value {
    match tree {
        Value::Array(entries) => Value::Array(
            entries
                .into_iter()
                .map(|(k, v)| {
                    let v = match v {
                        Value::Str(s) if s.starts_with("var:") => {
                            string_value(&convert_custom_properties(s.as_bytes()))
                        }
                        Value::Bytes(b) if b.starts_with(b"var:") => {
                            string_value(&convert_custom_properties(&b))
                        }
                        Value::Array(_) => resolve_custom_css_format(v),
                        other => other,
                    };
                    (k, v)
                })
                .collect(),
        ),
        other => other,
    }
}
