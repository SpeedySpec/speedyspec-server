//! Rendering the specification as JSON documents.

use serde_json::{Map, Value as Json, json};
use speedyspec_operation_contracts::{
    Kind, OperationSpec, Pagination, Schema, SchemaKind, TypeSpec,
};

use crate::json::to_json;

/// The JSON Schema 2020-12 form of `schema`. Named types become `{"$ref": "#/types/<name>"}`.
pub fn json_schema(schema: &Schema) -> Json {
    let mut out = Map::new();
    let typed = |out: &mut Map<String, Json>, name: &str| {
        let ty = if schema.nullable {
            json!([name, "null"])
        } else {
            json!(name)
        };
        out.insert("type".into(), ty);
    };
    match &schema.kind {
        SchemaKind::Any => {}
        SchemaKind::Boolean => typed(&mut out, "boolean"),
        SchemaKind::Integer { minimum, maximum } | SchemaKind::Number { minimum, maximum } => {
            let name = if matches!(schema.kind, SchemaKind::Integer { .. }) {
                "integer"
            } else {
                "number"
            };
            typed(&mut out, name);
            insert_number(&mut out, "minimum", *minimum);
            insert_number(&mut out, "maximum", *maximum);
        }
        SchemaKind::String {
            min_length,
            max_length,
            pattern,
            format,
        } => {
            typed(&mut out, "string");
            if let Some(n) = min_length {
                out.insert("minLength".into(), json!(n));
            }
            if let Some(n) = max_length {
                out.insert("maxLength".into(), json!(n));
            }
            if let Some(p) = pattern {
                out.insert("pattern".into(), json!(p));
            }
            if let Some(f) = format {
                out.insert("format".into(), json!(f.as_str()));
            }
        }
        SchemaKind::Binary { media_type } => {
            typed(&mut out, "string");
            out.insert("contentEncoding".into(), json!("base64"));
            if let Some(m) = media_type {
                out.insert("contentMediaType".into(), json!(m));
            }
        }
        SchemaKind::DateTime => {
            typed(&mut out, "string");
            out.insert("format".into(), json!("date-time"));
        }
        SchemaKind::Array {
            items,
            min_items,
            max_items,
            unique_items,
        } => {
            typed(&mut out, "array");
            out.insert("items".into(), json_schema(items));
            if let Some(n) = min_items {
                out.insert("minItems".into(), json!(n));
            }
            if let Some(n) = max_items {
                out.insert("maxItems".into(), json!(n));
            }
            if *unique_items {
                out.insert("uniqueItems".into(), json!(true));
            }
        }
        SchemaKind::Object {
            properties,
            additional_properties,
        } => {
            typed(&mut out, "object");
            let props: Map<String, Json> = properties
                .iter()
                .map(|(name, p)| (name.clone(), json_schema(&p.schema)))
                .collect();
            out.insert("properties".into(), Json::Object(props));
            let required: Vec<&String> = properties
                .iter()
                .filter(|(_, p)| p.required)
                .map(|(n, _)| n)
                .collect();
            if !required.is_empty() {
                out.insert("required".into(), json!(required));
            }
            if !additional_properties {
                out.insert("additionalProperties".into(), json!(false));
            }
        }
        SchemaKind::OneOf {
            variants,
            discriminator,
        } => {
            let mut rendered: Vec<Json> = variants.iter().map(json_schema).collect();
            if schema.nullable {
                rendered.push(json!({"type": "null"}));
            }
            out.insert("oneOf".into(), Json::Array(rendered));
            if let Some(d) = discriminator {
                out.insert("discriminator".into(), json!({"propertyName": d}));
            }
        }
        SchemaKind::Ref { type_name } => {
            let reference = json!({"$ref": format!("#/types/{type_name}")});
            if schema.nullable {
                out.insert("oneOf".into(), json!([reference, {"type": "null"}]));
            } else {
                out.insert("$ref".into(), reference["$ref"].clone());
            }
        }
    }
    if let Some(d) = &schema.description {
        out.insert("description".into(), json!(d));
    }
    if !schema.enum_values.is_empty() {
        out.insert(
            "enum".into(),
            Json::Array(schema.enum_values.iter().map(to_json).collect()),
        );
    }
    Json::Object(out)
}

fn insert_number(out: &mut Map<String, Json>, key: &str, value: Option<f64>) {
    if let Some(v) = value {
        let n = if v.fract() == 0.0 && v.abs() < 9.0e15 {
            json!(v as i64)
        } else {
            json!(v)
        };
        out.insert(key.into(), n);
    }
}

/// The document of one operation: its traits and its contracts' JSON Schemas.
pub fn operation_document(spec: &OperationSpec) -> Json {
    let kind = match spec.kind {
        Kind::Query => "query",
        Kind::Command => "command",
        Kind::Subscription => "subscription",
    };
    let pagination = spec.pagination.map(|p| match p {
        Pagination::Cursor => "cursor",
        Pagination::Offset => "offset",
    });
    json!({
        "kind": kind,
        "resource": spec.resource.as_ref().map(|r| json!({
            "type": r.type_name,
            "action": r.action.as_str(),
            "identity": r.identity,
        })),
        "idempotent": spec.idempotent,
        "destructive": spec.destructive,
        "pagination": pagination,
        "input": json_schema(&spec.input.schema),
        "input_type": spec.input.mapper.type_name(),
        "output": json_schema(&spec.output.schema),
        "output_type": spec.output.mapper.type_name(),
        "errors": spec.errors.iter().map(|e| json!({"code": e.code, "category": e.category.as_str()})).collect::<Vec<_>>(),
        "labels": {"title": spec.labels.title, "description": spec.labels.description},
        "deprecated": spec.deprecated,
        "transports": spec.transports.as_ref().map(|t| t.iter().collect::<Vec<_>>()),
    })
}

/// The document of one type: its fields as JSON Schema properties, and its relations.
pub fn type_document(spec: &TypeSpec) -> Json {
    let object = Schema::of(SchemaKind::Object {
        properties: spec.fields.clone(),
        additional_properties: true,
    });
    let rendered = json_schema(&object);
    json!({
        "properties": rendered["properties"],
        "required": rendered.get("required").cloned().unwrap_or_else(|| json!([])),
        "relations": spec.relations.iter().map(|r| json!({
            "field": r.field,
            "operation": r.operation,
            "args": r.args.iter().map(|(k, v)| (k.clone(), json!(v))).collect::<Map<String, Json>>(),
        })).collect::<Vec<_>>(),
    })
}
