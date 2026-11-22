//! The facade's descriptor arrays, turned into specifications.
//!
//! `OperationBuilder::register()` and `TypeBuilder::register()` in PHP pass plain arrays (internal to the facade;
//! plugins write builders). This module reads them into the Rust builders, runs the schema-versus-class check, and
//! collects every reason a definition is refused.

use std::rc::Rc;

use speedyspec_operation::{Operation, OperationBuilder, S, SchemaBuilder, Type};
use speedyspec_operation_contracts::{
    Action, Category, OperationSpec, Pagination, TypeCatalog, TypeSpec,
};
use speedyspec_php_ext::{Key, Value as PhpValue};

use crate::convert::from_php;
use crate::mapper::{PhpInputMapper, PhpOutputMapper};
use crate::permission::PhpPermission;
use crate::shape::{ClassShape, Side, check_shape};

/// An array's entries, or an error naming `what`.
fn entries<'a>(value: &'a PhpValue, what: &str) -> Result<&'a [(Key, PhpValue)], String> {
    match value {
        PhpValue::Array(items) => Ok(items),
        _ => Err(format!("{what} must be an array")),
    }
}

fn get(items: &[(Key, PhpValue)], key: &str) -> PhpValue {
    items
        .iter()
        .find(|(k, _)| *k == Key::Str(key.to_owned()))
        .map_or(PhpValue::Null, |(_, v)| v.clone())
}

fn text(value: &PhpValue, what: &str) -> Result<String, String> {
    match value {
        PhpValue::Str(s) => Ok(s.clone()),
        _ => Err(format!("{what} must be a string")),
    }
}

fn optional_text(value: &PhpValue, what: &str) -> Result<Option<String>, String> {
    match value {
        PhpValue::Null => Ok(None),
        other => text(other, what).map(Some),
    }
}

fn texts(value: &PhpValue, what: &str) -> Result<Vec<String>, String> {
    entries(value, what)?
        .iter()
        .map(|(_, v)| text(v, what))
        .collect()
}

fn flag(value: &PhpValue) -> bool {
    matches!(value, PhpValue::Bool(true))
}

/// A schema from its JSON Schema array.
pub fn schema(value: &PhpValue, at: &str) -> Result<SchemaBuilder, String> {
    let neutral = from_php(value).map_err(|e| format!("{at}: {}", e.message))?;
    S::from_value(&neutral).map_err(|e| format!("{at}: {e}"))
}

fn action(name: &str) -> Action {
    match name {
        "list" => Action::List,
        "get" => Action::Get,
        "create" => Action::Create,
        "update" => Action::Update,
        "delete" => Action::Delete,
        other => Action::Custom(other.to_owned()),
    }
}

/// One side of an operation's contract: the builder with it set, and any shape problems.
fn contract(
    builder: OperationBuilder,
    value: &PhpValue,
    side: Side,
    name: &str,
    types: &dyn TypeCatalog,
    problems: &mut Vec<String>,
) -> Result<OperationBuilder, String> {
    let label = match side {
        Side::Input => "input",
        Side::Output => "output",
    };
    if matches!(value, PhpValue::Null) {
        return Ok(builder);
    }
    let items = entries(value, &format!("{name}'s {label}"))?;
    let class = optional_text(&get(items, "class"), "a value object class")?;
    let schema = schema(&get(items, "schema"), &format!("{name}.{label}"))?;
    if class.is_some() {
        let shape = ClassShape::from_php(&get(items, "shape"))?;
        problems.extend(check_shape(&schema.clone().build(), &shape, side, types));
    }
    Ok(match side {
        Side::Input => builder.input_with(schema, Rc::new(PhpInputMapper { class })),
        Side::Output => builder.output_with(schema, Rc::new(PhpOutputMapper { class })),
    })
}

/// The operation a descriptor describes, and its permission callback (`None` for a public operation). `types`
/// resolves type references for the shape check.
pub fn operation(
    value: &PhpValue,
    types: &dyn TypeCatalog,
) -> Result<(OperationSpec, Option<PhpValue>), Vec<String>> {
    let mut problems = Vec::new();
    let (builder, permission) = read(value, types, &mut problems).map_err(|one| vec![one])?;
    match builder.build() {
        Ok(spec) if problems.is_empty() => Ok((spec, permission)),
        Ok(_) => Err(problems),
        Err(err) => {
            problems.insert(0, err.to_string());
            Err(problems)
        }
    }
}

/// The builder a descriptor describes; shape problems go to `problems`, a malformed descriptor is the error.
fn read(
    value: &PhpValue,
    types: &dyn TypeCatalog,
    problems: &mut Vec<String>,
) -> Result<(OperationBuilder, Option<PhpValue>), String> {
    let items = entries(value, "an operation descriptor")?;
    let name = text(&get(items, "name"), "the operation name")?;
    let mut builder = match text(&get(items, "kind"), "the operation kind")?.as_str() {
        "query" => Operation::query(&name),
        "command" => Operation::command(&name),
        "subscription" => Operation::subscription(&name),
        other => return Err(format!("{name}: unknown operation kind {other}")),
    };
    if let PhpValue::Array(resource) = &get(items, "resource") {
        let [(_, type_name), (_, act), (_, identity)] = resource.as_slice() else {
            return Err(format!("{name}: resource must be [type, action, identity]"));
        };
        let identity = texts(identity, "an identity field")?;
        let identity: Vec<&str> = identity.iter().map(String::as_str).collect();
        builder = builder.resource(
            &text(type_name, "the resource type")?,
            action(&text(act, "the resource action")?),
            &identity,
        );
    }
    if flag(&get(items, "idempotent")) {
        builder = builder.idempotent();
    }
    if flag(&get(items, "destructive")) {
        builder = builder.destructive();
    }
    match optional_text(&get(items, "paginated"), "the pagination style")?.as_deref() {
        None => {}
        Some("cursor") => builder = builder.paginated(Pagination::Cursor),
        Some("offset") => builder = builder.paginated(Pagination::Offset),
        Some(other) => return Err(format!("{name}: unknown pagination style {other}")),
    }
    builder = contract(
        builder,
        &get(items, "input"),
        Side::Input,
        &name,
        types,
        problems,
    )?;
    builder = contract(
        builder,
        &get(items, "output"),
        Side::Output,
        &name,
        types,
        problems,
    )?;
    for (_, error) in entries(&get(items, "errors"), "errors")? {
        let error = entries(error, "an error")?;
        let [(_, code), (_, category)] = error else {
            return Err(format!("{name}: an error must be [code, category]"));
        };
        let category = text(category, "an error category")?;
        let category = Category::parse(&category)
            .ok_or_else(|| format!("{name}: unknown error category {category}"))?;
        builder = builder.error(&text(code, "an error code")?, category);
    }
    let permission = match get(items, "permission") {
        PhpValue::Null => None,
        callable => Some(callable),
    };
    if permission.is_some() {
        builder = builder.permission_check(Rc::new(PhpPermission {
            operation: name.clone(),
        }));
    } else if flag(&get(items, "public")) {
        builder = builder.public();
    }
    if let PhpValue::Array(label) = &get(items, "label") {
        let [(_, title), (_, description)] = label.as_slice() else {
            return Err(format!("{name}: label must be [title, description]"));
        };
        builder = builder.label(
            &text(title, "a title")?,
            &text(description, "a description")?,
        );
    }
    if let Some(reason) = optional_text(&get(items, "deprecated"), "the deprecation reason")? {
        builder = builder.deprecated(&reason);
    }
    if !matches!(&get(items, "transports"), PhpValue::Null) {
        let names = texts(&get(items, "transports"), "a transport name")?;
        let names: Vec<&str> = names.iter().map(String::as_str).collect();
        builder = builder.transports(&names);
    }
    Ok((builder, permission))
}

/// The type a descriptor describes.
pub fn type_spec(value: &PhpValue) -> Result<TypeSpec, String> {
    let items = entries(value, "a type descriptor")?;
    let name = text(&get(items, "name"), "the type name")?;
    let mut builder = Type::resource(&name);
    for (key, field) in entries(&get(items, "fields"), "fields")? {
        let Key::Str(field_name) = key else {
            return Err(format!("{name}: field names must be strings"));
        };
        let field = entries(field, "a field")?;
        let mut schema = schema(&get(field, "schema"), &format!("{name}.{field_name}"))?;
        if flag(&get(field, "required")) {
            schema = schema.required();
        }
        builder = builder.field(field_name, schema);
    }
    for (_, relation) in entries(&get(items, "relations"), "relations")? {
        let relation = entries(relation, "a relation")?;
        let [(_, field), (_, operation), (_, args)] = relation else {
            return Err(format!(
                "{name}: a relation must be [field, operation, args]"
            ));
        };
        let args = entries(args, "relation args")?
            .iter()
            .map(|(k, v)| match k {
                Key::Str(k) => text(v, "a relation source field").map(|v| (k.clone(), v)),
                Key::Int(_) => Err(format!(
                    "{name}: relation args map input fields to type fields"
                )),
            })
            .collect::<Result<Vec<_>, _>>()?;
        builder = builder.relation(
            &text(field, "a relation field")?,
            &text(operation, "a relation operation")?,
            args.iter().map(|(k, v)| (k.as_str(), v.as_str())),
        );
    }
    Ok(builder.build())
}
