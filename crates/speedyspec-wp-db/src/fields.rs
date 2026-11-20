//! Field formats and the text WordPress's insert, update and delete helpers build.

use indexmap::IndexMap;
use speedyspec_hooks::Value;

/// How a column's length is counted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LengthKind {
    Byte,
    Char,
}

/// A column's maximum length.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ColLength {
    pub kind: LengthKind,
    pub length: u64,
}

/// One field of a helper's data, as `process_fields` builds it.
#[derive(Debug, Clone, PartialEq)]
pub struct Field {
    pub name: String,
    pub value: Value,
    pub format: String,
    pub charset: Option<String>,
    pub length: Option<ColLength>,
}

/// `wpdb::process_field_formats` over `$wpdb->field_types`.
pub struct FieldFormats<'a> {
    pub field_types: &'a IndexMap<String, String>,
}

/// PHP's truthiness of a format string.
fn truthy(format: &str) -> bool {
    !format.is_empty() && format != "0"
}

impl FieldFormats<'_> {
    /// Each field takes the next format, else the first one; with no formats, `field_types`, else `%s`.
    pub fn process_field_formats(
        &self,
        data: &[(String, Value)],
        format: Option<&[String]>,
    ) -> Vec<Field> {
        let format = format.filter(|f| !f.is_empty());
        let mut remaining = format.map(|f| f.iter()).into_iter().flatten();
        let first = format.and_then(|f| f.first()).cloned().unwrap_or_default();
        data.iter()
            .map(|(name, value)| {
                let chosen = match format {
                    Some(_) => match remaining.next() {
                        Some(next) if truthy(next) => next.clone(),
                        _ => first.clone(),
                    },
                    None => self
                        .field_types
                        .get(name)
                        .cloned()
                        .unwrap_or_else(|| "%s".into()),
                };
                Field {
                    name: name.clone(),
                    value: value.clone(),
                    format: chosen,
                    charset: None,
                    length: None,
                }
            })
            .collect()
    }
}

fn assignments(fields: &[Field], args: &mut Vec<Value>, null: &str) -> Vec<String> {
    fields
        .iter()
        .map(|f| {
            if matches!(f.value, Value::Null) {
                format!("`{}`{null}", f.name)
            } else {
                args.push(f.value.clone());
                format!("`{}` = {}", f.name, f.format)
            }
        })
        .collect()
}

/// `_insert_replace_helper`'s text before `prepare()`, and the values left to prepare.
pub fn insert_sql(table: &str, fields: &[Field], verb: &str) -> (String, Vec<Value>) {
    let mut args = Vec::new();
    let formats: Vec<String> = fields
        .iter()
        .map(|f| {
            if matches!(f.value, Value::Null) {
                "NULL".to_string()
            } else {
                args.push(f.value.clone());
                f.format.clone()
            }
        })
        .collect();
    let names: Vec<&str> = fields.iter().map(|f| f.name.as_str()).collect();
    (
        format!(
            "{verb} INTO `{table}` (`{}`) VALUES ({})",
            names.join("`, `"),
            formats.join(", ")
        ),
        args,
    )
}

/// `update()`'s text before `prepare()`.
pub fn update_sql(table: &str, set: &[Field], conditions: &[Field]) -> (String, Vec<Value>) {
    let mut args = Vec::new();
    let fields = assignments(set, &mut args, " = NULL");
    let conditions = assignments(conditions, &mut args, " IS NULL");
    (
        format!(
            "UPDATE `{table}` SET {} WHERE {}",
            fields.join(", "),
            conditions.join(" AND ")
        ),
        args,
    )
}

/// `delete()`'s text before `prepare()`.
pub fn delete_sql(table: &str, conditions: &[Field]) -> (String, Vec<Value>) {
    let mut args = Vec::new();
    let conditions = assignments(conditions, &mut args, " IS NULL");
    (
        format!("DELETE FROM `{table}` WHERE {}", conditions.join(" AND ")),
        args,
    )
}
