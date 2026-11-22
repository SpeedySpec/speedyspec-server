//! The schema-versus-class registration check.
//!
//! PHP reflects a value object class into a [`ClassShape`] (`Internal\Shape::of`): for an input class its constructor
//! parameters, which the mapper fills by name; for an output class its public properties, which the mapper reads. This
//! module compares that shape with the operation's schema, so a class and its schema cannot silently disagree.

use indexmap::IndexMap;
use speedyspec_operation_contracts::{Property, Schema, SchemaKind, TypeCatalog};
use speedyspec_php_ext::{Key, Value as PhpValue};

/// A value object class as PHP reflection sees it.
#[derive(Clone, Debug, PartialEq)]
pub struct ClassShape {
    pub class: String,
    pub members: Vec<Member>,
}

/// A constructor parameter (input side) or public property (output side).
#[derive(Clone, Debug, PartialEq)]
pub struct Member {
    pub name: String,
    /// Lowercased PHP type names, `null` included for nullable types; empty when untyped.
    pub types: Vec<String>,
    pub nullable: bool,
    pub has_default: bool,
}

/// Which side of the contract a class is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Input,
    Output,
}

fn field<'a>(items: &'a [(Key, PhpValue)], name: &str) -> Option<&'a PhpValue> {
    items
        .iter()
        .find(|(k, _)| *k == Key::Str(name.to_owned()))
        .map(|(_, v)| v)
}

fn text(value: Option<&PhpValue>, what: &str) -> Result<String, String> {
    match value {
        Some(PhpValue::Str(s)) => Ok(s.clone()),
        _ => Err(format!("{what} must be a string")),
    }
}

impl ClassShape {
    /// Reads `['class' => string, 'members' => [['name', 'types', 'nullable', 'default'], …]]`.
    pub fn from_php(value: &PhpValue) -> Result<ClassShape, String> {
        let value = value.read();
        let PhpValue::Array(items) = value.as_ref() else {
            return Err("a class shape must be an array".into());
        };
        let class = text(field(items, "class"), "the shape's class")?;
        let Some(PhpValue::Array(members)) = field(items, "members") else {
            return Err("the shape's members must be an array".into());
        };
        let members = members
            .iter()
            .map(|(_, member)| {
                let PhpValue::Array(m) = member else {
                    return Err("a shape member must be an array".to_owned());
                };
                let types = match field(m, "types") {
                    Some(PhpValue::Array(types)) => types
                        .iter()
                        .map(|(_, t)| text(Some(t), "a member type").map(|t| t.to_lowercase()))
                        .collect::<Result<Vec<_>, _>>()?,
                    _ => return Err("a member's types must be an array".into()),
                };
                Ok(Member {
                    name: text(field(m, "name"), "a member name")?,
                    types,
                    nullable: matches!(field(m, "nullable"), Some(PhpValue::Bool(true))),
                    has_default: matches!(field(m, "default"), Some(PhpValue::Bool(true))),
                })
            })
            .collect::<Result<_, String>>()?;
        Ok(ClassShape { class, members })
    }
}

const BUILTIN: &[&str] = &[
    "int", "float", "string", "bool", "true", "false", "array", "iterable", "object", "mixed",
    "null", "void", "never", "callable", "self", "static", "parent",
];

fn kind_name(kind: &SchemaKind) -> String {
    match kind {
        SchemaKind::Any => "any".into(),
        SchemaKind::Boolean => "boolean".into(),
        SchemaKind::Integer { .. } => "integer".into(),
        SchemaKind::Number { .. } => "number".into(),
        SchemaKind::String { .. } => "string".into(),
        SchemaKind::DateTime => "date-time".into(),
        SchemaKind::Binary { .. } => "binary".into(),
        SchemaKind::Array { .. } => "array".into(),
        SchemaKind::Object { .. } => "object".into(),
        SchemaKind::OneOf { .. } => "oneOf".into(),
        SchemaKind::Ref { type_name } => format!("a {type_name}"),
    }
}

/// Whether the PHP type `php` (lowercased) can hold a value of `kind`.
fn accepts(kind: &SchemaKind, php: &str) -> bool {
    let class = !BUILTIN.contains(&php);
    match kind {
        SchemaKind::Any => true,
        SchemaKind::Boolean => matches!(php, "bool" | "true" | "false"),
        SchemaKind::Integer { .. } => php == "int",
        SchemaKind::Number { .. } => matches!(php, "float" | "int"),
        SchemaKind::String { .. } | SchemaKind::Binary { .. } => php == "string",
        SchemaKind::DateTime => {
            matches!(
                php,
                "datetimeinterface" | "datetimeimmutable" | "datetime" | "string"
            )
        }
        SchemaKind::Array { .. } => matches!(php, "array" | "iterable"),
        SchemaKind::Object { .. } | SchemaKind::Ref { .. } | SchemaKind::OneOf { .. } => {
            matches!(php, "array" | "object") || class
        }
    }
}

/// Every way `shape` disagrees with `schema`, as messages naming the class and member. Empty when they agree. A
/// reference to a type not registered yet is skipped: closing the registries reports it.
pub fn check_shape(
    schema: &Schema,
    shape: &ClassShape,
    side: Side,
    types: &dyn TypeCatalog,
) -> Vec<String> {
    let class = &shape.class;
    let referenced;
    let (properties, closed): (&IndexMap<String, Property>, bool) = match &schema.kind {
        SchemaKind::Object {
            properties,
            additional_properties,
        } => (properties, !additional_properties),
        SchemaKind::Ref { type_name } => match types.get_type(type_name) {
            Some(spec) => {
                referenced = spec;
                (&referenced.fields, false)
            }
            None => return Vec::new(),
        },
        other => {
            return vec![format!(
                "{class} needs an object schema, the schema is {}",
                kind_name(other)
            )];
        }
    };
    let mut problems = Vec::new();
    for (name, property) in properties {
        let Some(member) = shape.members.iter().find(|m| &m.name == name) else {
            match side {
                Side::Input => {
                    problems.push(format!("{class} has no constructor parameter ${name}"))
                }
                Side::Output if property.required => {
                    problems.push(format!("{class} has no public property ${name}"))
                }
                Side::Output => {}
            }
            continue;
        };
        let declared: Vec<&str> = member
            .types
            .iter()
            .map(String::as_str)
            .filter(|t| *t != "null")
            .collect();
        let untyped = declared.is_empty() || declared.contains(&"mixed");
        if !untyped && !declared.iter().any(|t| accepts(&property.schema.kind, t)) {
            problems.push(format!(
                "{class}::${name} is {}, the schema says {}",
                declared.join("|"),
                kind_name(&property.schema.kind)
            ));
        }
        if property.schema.nullable && !member.nullable {
            problems.push(format!(
                "{class}::${name} may be null in the schema but its type does not allow null"
            ));
        }
        if side == Side::Input && !property.required && !member.has_default && !member.nullable {
            problems.push(format!(
                "{class}::${name} is optional in the schema but has no default and is not nullable"
            ));
        }
    }
    for member in &shape.members {
        if properties.contains_key(&member.name) {
            continue;
        }
        let name = &member.name;
        match side {
            Side::Input if !member.has_default => problems.push(format!(
                "{class}::${name} is not in the schema and has no default"
            )),
            Side::Output if closed => {
                problems.push(format!("{class}::${name} is not in the closed schema"))
            }
            _ => {}
        }
    }
    problems
}
