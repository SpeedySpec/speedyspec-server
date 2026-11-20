//! Names and literals as MySQL means them, read from `sqlparser`'s tree.

use sqlparser::ast::{Expr, ObjectName, Value};

/// A table or variable name without its quotes, parts joined with `.`.
pub fn object_name(name: &ObjectName) -> String {
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

/// A name as MySQL text: bare when it is a plain, non-keyword identifier, else backquoted.
pub fn sql_ident(name: &str) -> String {
    let plain = name
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$');
    let keyword = sqlparser::keywords::ALL_KEYWORDS
        .binary_search(&name.to_ascii_uppercase().as_str())
        .is_ok();
    if plain && !keyword {
        name.to_string()
    } else {
        format!("`{}`", name.replace('`', "``"))
    }
}

/// A string as a MySQL literal.
pub fn sql_literal(text: &str) -> String {
    format!("'{}'", text.replace('\\', "\\\\").replace('\'', "''"))
}

/// An optional string as a MySQL literal, `NULL` for none.
pub(crate) fn sql_opt(text: Option<&str>) -> String {
    text.map_or_else(|| "NULL".to_string(), sql_literal)
}
