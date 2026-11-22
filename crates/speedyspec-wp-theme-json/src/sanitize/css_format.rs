use speedyspec_hooks::Value;

use crate::codec::string_value;

/// `convert_custom_properties`: `var:preset|color|red` becomes `var(--wp--preset--color--red)`.
pub fn convert_custom_properties(value: &[u8]) -> Vec<u8> {
    let Some(rest) = value.strip_prefix(b"var:") else {
        return value.to_vec();
    };
    let mut out = b"var(--wp--".to_vec();
    for &c in rest {
        if c == b'|' {
            out.extend_from_slice(b"--");
        } else {
            out.push(c);
        }
    }
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
