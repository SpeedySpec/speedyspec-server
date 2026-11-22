//! Validating neutral values against schemas.

use std::net::IpAddr;
use std::sync::LazyLock;

use indexmap::IndexMap;
use regex::Regex;
use speedyspec_operation_contracts::{
    Property, Schema, SchemaKind, StringFormat, TypeCatalog, Value,
};

/// One way a value breaks its schema.
#[derive(Clone, Debug, PartialEq)]
pub struct Violation {
    /// Where: `""` for the root, `title`, `tags[0]`, `author.name`.
    pub path: String,
    /// The JSON Schema keyword broken (`type`, `required`, `maxLength`, …).
    pub rule: &'static str,
    pub message: String,
}

/// Checks `value` against `schema`, collecting every violation.
pub fn validate(
    schema: &Schema,
    value: &Value,
    types: &dyn TypeCatalog,
) -> Result<(), Vec<Violation>> {
    let mut out = Vec::new();
    check(schema, value, "", types, &mut out);
    if out.is_empty() { Ok(()) } else { Err(out) }
}

/// Violations as a neutral list of `{path, rule, message}` maps, for error details.
pub fn violations_value(violations: &[Violation]) -> Value {
    Value::List(
        violations
            .iter()
            .map(|v| {
                Value::map([
                    ("path", Value::from(v.path.as_str())),
                    ("rule", Value::from(v.rule)),
                    ("message", Value::from(v.message.as_str())),
                ])
            })
            .collect(),
    )
}

fn push(out: &mut Vec<Violation>, path: &str, rule: &'static str, message: String) {
    out.push(Violation {
        path: path.to_owned(),
        rule,
        message,
    });
}

fn child(path: &str, name: &str) -> String {
    if path.is_empty() {
        name.to_owned()
    } else {
        format!("{path}.{name}")
    }
}

fn wrong_type(out: &mut Vec<Violation>, path: &str, expected: &str, value: &Value) {
    push(
        out,
        path,
        "type",
        format!("expected {expected}, got {}", value.kind_name()),
    );
}

fn check(
    schema: &Schema,
    value: &Value,
    path: &str,
    types: &dyn TypeCatalog,
    out: &mut Vec<Violation>,
) {
    if matches!(value, Value::Null) {
        if !schema.nullable && schema.kind != SchemaKind::Any {
            wrong_type(out, path, "a non-null value", value);
        }
        return;
    }
    if !schema.enum_values.is_empty() && !schema.enum_values.contains(value) {
        push(out, path, "enum", "not one of the allowed values".into());
    }
    match &schema.kind {
        SchemaKind::Any => {}
        SchemaKind::Boolean => {
            if !matches!(value, Value::Bool(_)) {
                wrong_type(out, path, "boolean", value);
            }
        }
        SchemaKind::Integer { minimum, maximum } => match as_integer(value) {
            Some(n) => bounds(n, *minimum, *maximum, path, out),
            None => wrong_type(out, path, "integer", value),
        },
        SchemaKind::Number { minimum, maximum } => match value {
            Value::Int(i) => bounds(*i as f64, *minimum, *maximum, path, out),
            Value::Float(f) => bounds(*f, *minimum, *maximum, path, out),
            _ => wrong_type(out, path, "number", value),
        },
        SchemaKind::String {
            min_length,
            max_length,
            pattern,
            format,
        } => {
            let Value::String(s) = value else {
                return wrong_type(out, path, "string", value);
            };
            let len = s.chars().count();
            if let Some(min) = min_length.filter(|min| len < *min) {
                push(
                    out,
                    path,
                    "minLength",
                    format!("shorter than {min} characters"),
                );
            }
            if let Some(max) = max_length.filter(|max| len > *max) {
                push(
                    out,
                    path,
                    "maxLength",
                    format!("longer than {max} characters"),
                );
            }
            if let Some(p) = pattern {
                match Regex::new(p) {
                    Ok(re) if re.is_match(s) => {}
                    Ok(_) => push(out, path, "pattern", format!("does not match {p}")),
                    Err(e) => push(out, path, "pattern", format!("invalid pattern {p}: {e}")),
                }
            }
            if let Some(f) = format
                && !format_ok(*f, s)
            {
                push(out, path, "format", format!("not a valid {}", f.as_str()));
            }
        }
        SchemaKind::Binary { .. } => {
            if !matches!(value, Value::Binary(_)) {
                wrong_type(out, path, "binary", value);
            }
        }
        SchemaKind::DateTime => {
            if !matches!(value, Value::DateTime(_)) {
                wrong_type(out, path, "datetime", value);
            }
        }
        SchemaKind::Array {
            items,
            min_items,
            max_items,
            unique_items,
        } => {
            let Value::List(list) = value else {
                return wrong_type(out, path, "array", value);
            };
            if let Some(min) = min_items.filter(|min| list.len() < *min) {
                push(out, path, "minItems", format!("fewer than {min} items"));
            }
            if let Some(max) = max_items.filter(|max| list.len() > *max) {
                push(out, path, "maxItems", format!("more than {max} items"));
            }
            if *unique_items && list.iter().enumerate().any(|(i, a)| list[..i].contains(a)) {
                push(out, path, "uniqueItems", "items are not unique".into());
            }
            for (i, item) in list.iter().enumerate() {
                check(items, item, &format!("{path}[{i}]"), types, out);
            }
        }
        SchemaKind::Object {
            properties,
            additional_properties,
        } => object(properties, *additional_properties, value, path, types, out),
        SchemaKind::Ref { type_name } => match types.get_type(type_name) {
            Some(spec) => object(&spec.fields, true, value, path, types, out),
            None => push(out, path, "ref", format!("unknown type {type_name}")),
        },
        SchemaKind::OneOf {
            variants,
            discriminator,
        } => one_of(variants, discriminator.as_deref(), value, path, types, out),
    }
}

fn as_integer(value: &Value) -> Option<f64> {
    match value {
        Value::Int(i) => Some(*i as f64),
        Value::Float(f) if f.is_finite() && f.fract() == 0.0 => Some(*f),
        _ => None,
    }
}

fn bounds(
    n: f64,
    minimum: Option<f64>,
    maximum: Option<f64>,
    path: &str,
    out: &mut Vec<Violation>,
) {
    if let Some(min) = minimum.filter(|min| n < *min) {
        push(out, path, "minimum", format!("less than {min}"));
    }
    if let Some(max) = maximum.filter(|max| n > *max) {
        push(out, path, "maximum", format!("greater than {max}"));
    }
}

fn object(
    properties: &IndexMap<String, Property>,
    additional: bool,
    value: &Value,
    path: &str,
    types: &dyn TypeCatalog,
    out: &mut Vec<Violation>,
) {
    let Value::Map(map) = value else {
        return wrong_type(out, path, "object", value);
    };
    for (name, property) in properties {
        match map.get(name) {
            Some(v) => check(&property.schema, v, &child(path, name), types, out),
            None if property.required => {
                push(out, &child(path, name), "required", "is required".into())
            }
            None => {}
        }
    }
    if !additional {
        for name in map.keys().filter(|k| !properties.contains_key(*k)) {
            push(
                out,
                &child(path, name),
                "additionalProperties",
                "is not allowed".into(),
            );
        }
    }
}

fn one_of(
    variants: &[Schema],
    discriminator: Option<&str>,
    value: &Value,
    path: &str,
    types: &dyn TypeCatalog,
    out: &mut Vec<Violation>,
) {
    if let Some(key) = discriminator {
        let chosen = value
            .get(key)
            .and_then(|tag| variants.iter().find(|v| names_tag(v, key, tag, types)));
        match chosen {
            Some(variant) => check(variant, value, path, types, out),
            None => push(out, path, "oneOf", format!("{key} does not name a variant")),
        }
        return;
    }
    let passing = variants
        .iter()
        .filter(|v| {
            let mut scratch = Vec::new();
            check(v, value, path, types, &mut scratch);
            scratch.is_empty()
        })
        .count();
    if passing != 1 {
        push(
            out,
            path,
            "oneOf",
            format!("matches {passing} variants, not exactly one"),
        );
    }
}

/// Whether `variant` declares `key` with an enum containing `tag`.
fn names_tag(variant: &Schema, key: &str, tag: &Value, types: &dyn TypeCatalog) -> bool {
    let has = |props: &IndexMap<String, Property>| {
        props
            .get(key)
            .is_some_and(|p| p.schema.enum_values.contains(tag))
    };
    match &variant.kind {
        SchemaKind::Object { properties, .. } => has(properties),
        SchemaKind::Ref { type_name } => types.get_type(type_name).is_some_and(|t| has(&t.fields)),
        _ => false,
    }
}

static EMAIL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[^@\s]+@[^@\s]+\.[^@\s]+$").unwrap());
static URI: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[A-Za-z][A-Za-z0-9+.-]*:").unwrap());
static UUID: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$")
        .unwrap()
});
static HOSTNAME: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(?:[A-Za-z0-9](?:[A-Za-z0-9-]{0,61}[A-Za-z0-9])?)(?:\.[A-Za-z0-9](?:[A-Za-z0-9-]{0,61}[A-Za-z0-9])?)*$")
        .unwrap()
});

fn format_ok(format: StringFormat, s: &str) -> bool {
    match format {
        StringFormat::Email => EMAIL.is_match(s),
        StringFormat::Uri => URI.is_match(s),
        StringFormat::Uuid => UUID.is_match(s),
        StringFormat::Date => {
            chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").is_ok() && s.len() == 10
        }
        StringFormat::Hostname => s.len() <= 253 && HOSTNAME.is_match(s),
        StringFormat::Ip => s.parse::<IpAddr>().is_ok(),
    }
}

/// `value` with every integral float under an `integer` schema turned into an integer, as WordPress casts after
/// `rest_is_integer`. Run before [`validate`] so enums and hydrators see integers. One-of branches are left alone.
pub fn normalize(schema: &Schema, value: &Value, types: &dyn TypeCatalog) -> Value {
    match (&schema.kind, value) {
        (SchemaKind::Integer { .. }, Value::Float(f))
            if f.is_finite()
                && f.fract() == 0.0
                && *f >= i64::MIN as f64
                && *f < i64::MAX as f64 =>
        {
            Value::Int(*f as i64)
        }
        (SchemaKind::Array { items, .. }, Value::List(list)) => Value::List(
            list.iter()
                .map(|item| normalize(items, item, types))
                .collect(),
        ),
        (SchemaKind::Object { properties, .. }, Value::Map(_)) => {
            normalize_object(properties, value, types)
        }
        (SchemaKind::Ref { type_name }, Value::Map(_)) => match types.get_type(type_name) {
            Some(spec) => normalize_object(&spec.fields, value, types),
            None => value.clone(),
        },
        _ => value.clone(),
    }
}

fn normalize_object(
    properties: &IndexMap<String, Property>,
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
                    Some(p) => normalize(&p.schema, v, types),
                    None => v.clone(),
                };
                (k.clone(), v)
            })
            .collect(),
    )
}
