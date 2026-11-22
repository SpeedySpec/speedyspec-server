//! CSS declarations and rulesets, as `WP_Theme_JSON` builds them.

use speedyspec_hooks::Value;

use crate::php::{to_number, to_php_string};

/// One `name: value` pair. The value stays a PHP value: `to_ruleset` writes numbers and strings and skips the
/// rest, as WordPress does.
#[derive(Clone, Debug, PartialEq)]
pub struct Declaration {
    /// The CSS property.
    pub name: String,
    /// The value.
    pub value: Value,
}

impl Declaration {
    /// A declaration.
    pub fn new(name: impl Into<String>, value: Value) -> Declaration {
        Declaration {
            name: name.into(),
            value,
        }
    }

    /// A declaration with a string value.
    pub fn text(name: impl Into<String>, value: impl Into<String>) -> Declaration {
        Declaration::new(name, Value::Str(value.into()))
    }
}

/// `WP_Theme_JSON::to_ruleset( $selector, $declarations )`: `selector{name: value;…}`, or nothing when there are
/// no declarations.
pub fn to_ruleset(selector: &str, declarations: &[Declaration]) -> Vec<u8> {
    if declarations.is_empty() {
        return Vec::new();
    }
    let mut block = Vec::new();
    for d in declarations {
        let value = match &d.value {
            v if to_number(v).is_some() => to_php_string(v),
            Value::Str(s) => s.as_bytes().to_vec(),
            Value::Bytes(b) => b.clone(),
            _ => continue,
        };
        block.extend_from_slice(d.name.as_bytes());
        block.extend_from_slice(b": ");
        block.extend_from_slice(&value);
        block.push(b';');
    }
    let mut out = selector.as_bytes().to_vec();
    out.push(b'{');
    out.extend_from_slice(&block);
    out.push(b'}');
    out
}
