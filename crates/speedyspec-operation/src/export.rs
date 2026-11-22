//! Rendering the specification as JSON documents.

use serde_json::{Map, Value as Json, json};
use speedyspec_operation_contracts::{Schema, SchemaKind};

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
