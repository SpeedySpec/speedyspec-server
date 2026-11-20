//! Names and literals as MySQL means them, read from `sqlparser`'s tree.

use sqlparser::ast::{Expr, ObjectName, Value};

/// A table or variable name without its quotes, parts joined with `.`.
pub(crate) fn object_name(name: &ObjectName) -> String {
    name.0
        .iter()
        .filter_map(|p| p.as_ident())
        .map(|i| i.value.clone())
        .collect::<Vec<_>>()
        .join(".")
}

/// A literal's value as text (a string's contents, a number's digits); any other expression as it prints.
pub(crate) fn literal_text(expr: &Expr) -> String {
    match expr {
        Expr::Value(v) => match &v.value {
            Value::SingleQuotedString(s) | Value::DoubleQuotedString(s) => s.clone(),
            Value::Number(n, _) => n.clone(),
            Value::Boolean(b) => {
                if *b {
                    "1".into()
                } else {
                    "0".into()
                }
            }
            other => other.to_string(),
        },
        Expr::Identifier(ident) => ident.value.clone(),
        other => other.to_string(),
    }
}
