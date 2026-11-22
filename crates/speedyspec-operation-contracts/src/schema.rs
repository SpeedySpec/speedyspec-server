//! Schemas: what an operation accepts and returns, protocol-neutral. A JSON Schema 2020-12 subset.

use indexmap::IndexMap;

use crate::Value;

/// A schema value object. Built with `speedyspec_operation::S`; rendered as JSON Schema by
/// `speedyspec_operation::export::json_schema`.
#[derive(Clone, Debug, PartialEq)]
pub struct Schema {
    pub kind: SchemaKind,
    /// Whether `null` is accepted as well.
    pub nullable: bool,
    pub description: Option<String>,
    /// When not empty, the only values accepted.
    pub enum_values: Vec<Value>,
}

impl Schema {
    pub fn of(kind: SchemaKind) -> Schema {
        Schema {
            kind,
            nullable: false,
            description: None,
            enum_values: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum SchemaKind {
    /// Anything.
    Any,
    Boolean,
    Integer {
        minimum: Option<f64>,
        maximum: Option<f64>,
    },
    Number {
        minimum: Option<f64>,
        maximum: Option<f64>,
    },
    String {
        min_length: Option<usize>,
        max_length: Option<usize>,
        pattern: Option<String>,
        format: Option<StringFormat>,
    },
    /// Bytes; on JSON wires a base64 string.
    Binary {
        media_type: Option<String>,
    },
    /// A date and time; on JSON wires an RFC 3339 string.
    DateTime,
    Array {
        items: Box<Schema>,
        min_items: Option<usize>,
        max_items: Option<usize>,
        unique_items: bool,
    },
    Object {
        properties: IndexMap<String, Property>,
        /// Whether properties not listed are accepted.
        additional_properties: bool,
    },
    OneOf {
        variants: Vec<Schema>,
        /// The property whose value names the variant.
        discriminator: Option<String>,
    },
    /// A named type from the type registry.
    Ref {
        type_name: String,
    },
}

/// An object property.
#[derive(Clone, Debug, PartialEq)]
pub struct Property {
    pub schema: Schema,
    pub required: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StringFormat {
    Email,
    Uri,
    Uuid,
    Date,
    Hostname,
    Ip,
}

impl StringFormat {
    /// The JSON Schema `format` name.
    pub fn as_str(&self) -> &'static str {
        match self {
            StringFormat::Email => "email",
            StringFormat::Uri => "uri",
            StringFormat::Uuid => "uuid",
            StringFormat::Date => "date",
            StringFormat::Hostname => "hostname",
            StringFormat::Ip => "ip",
        }
    }

    pub fn parse(name: &str) -> Option<StringFormat> {
        [
            StringFormat::Email,
            StringFormat::Uri,
            StringFormat::Uuid,
            StringFormat::Date,
            StringFormat::Hostname,
            StringFormat::Ip,
        ]
        .into_iter()
        .find(|f| f.as_str() == name)
    }
}
