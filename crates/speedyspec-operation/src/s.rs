//! The `S` schema builder.

use indexmap::IndexMap;
use serde_json::Value as Json;
use speedyspec_operation_contracts::{Property, Schema, SchemaKind, StringFormat, Value};

use crate::json::from_json;

/// Starts schemas. Each function returns a [`SchemaBuilder`]; chain constraints, then `build()`.
pub struct S;

impl S {
    pub fn any() -> SchemaBuilder {
        SchemaBuilder::of(SchemaKind::Any)
    }

    pub fn boolean() -> SchemaBuilder {
        SchemaBuilder::of(SchemaKind::Boolean)
    }

    pub fn integer() -> SchemaBuilder {
        SchemaBuilder::of(SchemaKind::Integer {
            minimum: None,
            maximum: None,
        })
    }

    pub fn number() -> SchemaBuilder {
        SchemaBuilder::of(SchemaKind::Number {
            minimum: None,
            maximum: None,
        })
    }

    pub fn string() -> SchemaBuilder {
        SchemaBuilder::of(SchemaKind::String {
            min_length: None,
            max_length: None,
            pattern: None,
            format: None,
        })
    }

    pub fn date_time() -> SchemaBuilder {
        SchemaBuilder::of(SchemaKind::DateTime)
    }

    /// Bytes of the given media type.
    pub fn binary(media_type: &str) -> SchemaBuilder {
        SchemaBuilder::of(SchemaKind::Binary {
            media_type: Some(media_type.to_owned()),
        })
    }

    /// Bytes of any media type.
    pub fn binary_any() -> SchemaBuilder {
        SchemaBuilder::of(SchemaKind::Binary { media_type: None })
    }

    pub fn array(items: SchemaBuilder) -> SchemaBuilder {
        SchemaBuilder::of(SchemaKind::Array {
            items: Box::new(items.build()),
            min_items: None,
            max_items: None,
            unique_items: false,
        })
    }

    /// An object with these properties, in order. Mark properties `required()` on their own builders.
    pub fn object<'a>(props: impl IntoIterator<Item = (&'a str, SchemaBuilder)>) -> SchemaBuilder {
        let properties = props
            .into_iter()
            .map(|(name, builder)| {
                let required = builder.required;
                (
                    name.to_owned(),
                    Property {
                        schema: builder.schema,
                        required,
                    },
                )
            })
            .collect();
        SchemaBuilder::of(SchemaKind::Object {
            properties,
            additional_properties: true,
        })
    }

    pub fn one_of(variants: impl IntoIterator<Item = SchemaBuilder>) -> SchemaBuilder {
        SchemaBuilder::of(SchemaKind::OneOf {
            variants: variants.into_iter().map(SchemaBuilder::build).collect(),
            discriminator: None,
        })
    }

    /// A named type from the type registry.
    pub fn reference(type_name: &str) -> SchemaBuilder {
        SchemaBuilder::of(SchemaKind::Ref {
            type_name: type_name.to_owned(),
        })
    }

    /// A schema from JSON Schema, limited to the keywords `export::json_schema` emits.
    pub fn raw(json: &Json) -> Result<SchemaBuilder, SchemaError> {
        parse(json).map(SchemaBuilder::from_schema)
    }
}

/// Why `S::raw` refused a JSON Schema.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum SchemaError {
    #[error("the JSON Schema keyword {keyword} is not supported")]
    Unsupported { keyword: String },
    #[error("invalid schema: {why}")]
    Invalid { why: String },
}

/// Builds one [`Schema`]. Constraints applied to the wrong kind of schema panic: that is a programming error, found at
/// registration.
#[derive(Clone, Debug)]
pub struct SchemaBuilder {
    schema: Schema,
    required: bool,
}

impl SchemaBuilder {
    fn of(kind: SchemaKind) -> SchemaBuilder {
        SchemaBuilder::from_schema(Schema::of(kind))
    }

    fn from_schema(schema: Schema) -> SchemaBuilder {
        SchemaBuilder {
            schema,
            required: false,
        }
    }

    /// As an object property, it must be present.
    pub fn required(mut self) -> Self {
        self.required = true;
        self
    }

    pub fn is_required(&self) -> bool {
        self.required
    }

    pub fn nullable(mut self) -> Self {
        self.schema.nullable = true;
        self
    }

    pub fn description(mut self, description: &str) -> Self {
        self.schema.description = Some(description.to_owned());
        self
    }

    pub fn enum_values(mut self, values: Vec<Value>) -> Self {
        self.schema.enum_values = values;
        self
    }

    pub fn minimum(mut self, value: f64) -> Self {
        match &mut self.schema.kind {
            SchemaKind::Integer { minimum, .. } | SchemaKind::Number { minimum, .. } => {
                *minimum = Some(value)
            }
            _ => misuse("minimum", "integer or number"),
        }
        self
    }

    pub fn maximum(mut self, value: f64) -> Self {
        match &mut self.schema.kind {
            SchemaKind::Integer { maximum, .. } | SchemaKind::Number { maximum, .. } => {
                *maximum = Some(value)
            }
            _ => misuse("maximum", "integer or number"),
        }
        self
    }

    pub fn min_length(mut self, value: usize) -> Self {
        match &mut self.schema.kind {
            SchemaKind::String { min_length, .. } => *min_length = Some(value),
            _ => misuse("min_length", "string"),
        }
        self
    }

    pub fn max_length(mut self, value: usize) -> Self {
        match &mut self.schema.kind {
            SchemaKind::String { max_length, .. } => *max_length = Some(value),
            _ => misuse("max_length", "string"),
        }
        self
    }

    pub fn pattern(mut self, value: &str) -> Self {
        match &mut self.schema.kind {
            SchemaKind::String { pattern, .. } => *pattern = Some(value.to_owned()),
            _ => misuse("pattern", "string"),
        }
        self
    }

    pub fn format(mut self, value: StringFormat) -> Self {
        match &mut self.schema.kind {
            SchemaKind::String { format, .. } => *format = Some(value),
            _ => misuse("format", "string"),
        }
        self
    }

    pub fn min_items(mut self, value: usize) -> Self {
        match &mut self.schema.kind {
            SchemaKind::Array { min_items, .. } => *min_items = Some(value),
            _ => misuse("min_items", "array"),
        }
        self
    }

    pub fn max_items(mut self, value: usize) -> Self {
        match &mut self.schema.kind {
            SchemaKind::Array { max_items, .. } => *max_items = Some(value),
            _ => misuse("max_items", "array"),
        }
        self
    }

    pub fn unique_items(mut self) -> Self {
        match &mut self.schema.kind {
            SchemaKind::Array { unique_items, .. } => *unique_items = true,
            _ => misuse("unique_items", "array"),
        }
        self
    }

    /// Properties not listed are refused.
    pub fn closed(mut self) -> Self {
        match &mut self.schema.kind {
            SchemaKind::Object {
                additional_properties,
                ..
            } => *additional_properties = false,
            _ => misuse("closed", "object"),
        }
        self
    }

    pub fn discriminator(mut self, property: &str) -> Self {
        match &mut self.schema.kind {
            SchemaKind::OneOf { discriminator, .. } => *discriminator = Some(property.to_owned()),
            _ => misuse("discriminator", "oneOf"),
        }
        self
    }

    pub fn build(self) -> Schema {
        self.schema
    }
}

fn misuse(method: &str, kinds: &str) -> ! {
    panic!("{method} applies to {kinds} schemas")
}

const KEYWORDS: &[&str] = &[
    "type",
    "description",
    "enum",
    "minimum",
    "maximum",
    "minLength",
    "maxLength",
    "pattern",
    "format",
    "contentEncoding",
    "contentMediaType",
    "items",
    "minItems",
    "maxItems",
    "uniqueItems",
    "properties",
    "required",
    "additionalProperties",
    "oneOf",
    "discriminator",
    "$ref",
];

fn invalid(why: impl Into<String>) -> SchemaError {
    SchemaError::Invalid { why: why.into() }
}

fn parse(json: &Json) -> Result<Schema, SchemaError> {
    let Json::Object(map) = json else {
        return Err(invalid("a schema must be an object"));
    };
    if let Some(keyword) = map.keys().find(|k| !KEYWORDS.contains(&k.as_str())) {
        return Err(SchemaError::Unsupported {
            keyword: keyword.clone(),
        });
    }
    let num = |key: &str| map.get(key).and_then(Json::as_f64);
    let size = |key: &str| -> Result<Option<usize>, SchemaError> {
        match map.get(key) {
            None => Ok(None),
            Some(v) => v
                .as_u64()
                .map(|n| Some(n as usize))
                .ok_or_else(|| invalid(format!("{key} must be a non-negative integer"))),
        }
    };
    let text = |key: &str| map.get(key).and_then(Json::as_str).map(str::to_owned);

    let (type_name, mut nullable) = match map.get("type") {
        None => (None, false),
        Some(Json::String(t)) => (Some(t.as_str()), false),
        Some(Json::Array(types)) => {
            let names: Vec<&str> = types.iter().filter_map(Json::as_str).collect();
            match names.as_slice() {
                [t, "null"] | ["null", t] => (Some(*t), true),
                _ => return Err(invalid("type lists must be [\"<type>\", \"null\"]")),
            }
        }
        Some(_) => return Err(invalid("type must be a string or a list")),
    };

    let kind = match type_name {
        None => {
            if let Some(reference) = text("$ref") {
                let type_name = reference
                    .strip_prefix("#/types/")
                    .ok_or_else(|| invalid(format!("$ref {reference} is not #/types/<name>")))?;
                SchemaKind::Ref {
                    type_name: type_name.to_owned(),
                }
            } else if let Some(Json::Array(variants)) = map.get("oneOf") {
                let mut parsed = Vec::new();
                for variant in variants {
                    if variant == &serde_json::json!({"type": "null"}) {
                        nullable = true;
                    } else {
                        parsed.push(parse(variant)?);
                    }
                }
                let discriminator = map
                    .get("discriminator")
                    .and_then(|d| d.get("propertyName"))
                    .and_then(Json::as_str)
                    .map(str::to_owned);
                if parsed.len() == 1 && discriminator.is_none() && nullable {
                    // How `json_schema` renders a nullable reference.
                    let mut only = parsed.remove(0);
                    only.nullable = true;
                    return Ok(with_common(only, map));
                }
                SchemaKind::OneOf {
                    variants: parsed,
                    discriminator,
                }
            } else {
                SchemaKind::Any
            }
        }
        Some("boolean") => SchemaKind::Boolean,
        Some("integer") => SchemaKind::Integer {
            minimum: num("minimum"),
            maximum: num("maximum"),
        },
        Some("number") => SchemaKind::Number {
            minimum: num("minimum"),
            maximum: num("maximum"),
        },
        Some("string") => {
            if text("contentEncoding").as_deref() == Some("base64") {
                SchemaKind::Binary {
                    media_type: text("contentMediaType"),
                }
            } else if text("format").as_deref() == Some("date-time") {
                SchemaKind::DateTime
            } else {
                let format = match text("format") {
                    None => None,
                    Some(name) => Some(
                        StringFormat::parse(&name)
                            .ok_or_else(|| invalid(format!("unknown format {name}")))?,
                    ),
                };
                SchemaKind::String {
                    min_length: size("minLength")?,
                    max_length: size("maxLength")?,
                    pattern: text("pattern"),
                    format,
                }
            }
        }
        Some("array") => SchemaKind::Array {
            items: Box::new(match map.get("items") {
                Some(items) => parse(items)?,
                None => Schema::of(SchemaKind::Any),
            }),
            min_items: size("minItems")?,
            max_items: size("maxItems")?,
            unique_items: map
                .get("uniqueItems")
                .and_then(Json::as_bool)
                .unwrap_or(false),
        },
        Some("object") => {
            let required: Vec<&str> = map
                .get("required")
                .and_then(Json::as_array)
                .map(|r| r.iter().filter_map(Json::as_str).collect())
                .unwrap_or_default();
            let mut properties = IndexMap::new();
            if let Some(Json::Object(props)) = map.get("properties") {
                for (name, schema) in props {
                    properties.insert(
                        name.clone(),
                        Property {
                            schema: parse(schema)?,
                            required: required.contains(&name.as_str()),
                        },
                    );
                }
            }
            SchemaKind::Object {
                properties,
                additional_properties: map
                    .get("additionalProperties")
                    .and_then(Json::as_bool)
                    .unwrap_or(true),
            }
        }
        Some(other) => return Err(invalid(format!("unsupported type {other}"))),
    };
    let mut schema = with_common(Schema::of(kind), map);
    schema.nullable = nullable;
    Ok(schema)
}

fn with_common(mut schema: Schema, map: &serde_json::Map<String, Json>) -> Schema {
    if let Some(description) = map.get("description").and_then(Json::as_str) {
        schema.description = Some(description.to_owned());
    }
    if let Some(Json::Array(values)) = map.get("enum") {
        schema.enum_values = values.iter().map(from_json).collect();
    }
    schema
}
