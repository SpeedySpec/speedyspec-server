//! Absent values that PHP writes as `null`.
//!
//! An unset optional property of a PHP value object reads back as `null`, and PHP callers often pass `null` for an
//! argument they mean to leave out. Where the schema says a property is optional and not nullable, a `null` can only
//! mean "absent", so it is removed before validation. A nullable property keeps its `null`.

use speedyspec_operation_contracts::{Property, Schema, SchemaKind, TypeCatalog, Value};

/// `value` with every `null` removed where its property is optional and not nullable, at any depth.
pub fn absent_nulls(schema: &Schema, value: &Value, types: &dyn TypeCatalog) -> Value {
    match (&schema.kind, value) {
        (SchemaKind::Object { properties, .. }, Value::Map(_)) => object(properties, value, types),
        (SchemaKind::Ref { type_name }, Value::Map(_)) => match types.get_type(type_name) {
            Some(spec) => object(&spec.fields, value, types),
            None => value.clone(),
        },
        (SchemaKind::Array { items, .. }, Value::List(list)) => Value::List(
            list.iter()
                .map(|item| absent_nulls(items, item, types))
                .collect(),
        ),
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
            .filter_map(|(key, v)| match properties.get(key) {
                Some(p) if matches!(v, Value::Null) && !p.required && !p.schema.nullable => None,
                Some(p) => Some((key.clone(), absent_nulls(&p.schema, v, types))),
                None => Some((key.clone(), v.clone())),
            })
            .collect(),
    )
}
