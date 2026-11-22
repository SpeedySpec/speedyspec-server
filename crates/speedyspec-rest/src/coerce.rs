//! Wire strings to the kinds their schema expects, after `rest_sanitize_value_from_schema`.

use speedyspec_operation_contracts::{Property, Schema, SchemaKind, TypeCatalog, Value};

/// `value` with strings turned into the kind `schema` expects where they read as one: a decimal string under
/// `integer`, a numeric string under `number`, `true`/`1` and `false`/`0`/`` under `boolean`, a comma-separated string
/// under `array` (each item coerced too). A string that does not read as the kind is left for validation to report.
pub fn coerce(schema: &Schema, value: &Value, types: &dyn TypeCatalog) -> Value {
    match (&schema.kind, value) {
        (SchemaKind::Integer { .. }, Value::String(s)) => s
            .trim()
            .parse::<i64>()
            .map_or_else(|_| value.clone(), Value::Int),
        (SchemaKind::Number { .. }, Value::String(s)) => match s.trim().parse::<f64>() {
            Ok(f) if f.is_finite() => Value::Float(f),
            _ => value.clone(),
        },
        (SchemaKind::Boolean, Value::String(s)) => match s.as_str() {
            "true" | "1" => Value::Bool(true),
            "false" | "0" | "" => Value::Bool(false),
            _ => value.clone(),
        },
        (SchemaKind::Array { items, .. }, Value::String(s)) => Value::List(
            s.split(',')
                .filter(|part| !part.is_empty())
                .map(|part| coerce(items, &Value::from(part.trim()), types))
                .collect(),
        ),
        (SchemaKind::Array { items, .. }, Value::List(list)) => {
            Value::List(list.iter().map(|item| coerce(items, item, types)).collect())
        }
        (SchemaKind::Object { properties, .. }, Value::Map(_)) => object(properties, value, types),
        (SchemaKind::Ref { type_name }, Value::Map(_)) => match types.get_type(type_name) {
            Some(spec) => object(&spec.fields, value, types),
            None => value.clone(),
        },
        _ => value.clone(),
    }
}

fn object(
    properties: &indexmap::IndexMap<String, Property>,
    value: &Value,
    types: &dyn TypeCatalog,
) -> Value {
    let Value::Map(map) = value else {
        return value.clone();
    };
    Value::Map(
        map.iter()
            .map(|(k, v)| {
                let v = match properties.get(k) {
                    Some(p) => coerce(&p.schema, v, types),
                    None => v.clone(),
                };
                (k.clone(), v)
            })
            .collect(),
    )
}
