//! The index routes: `GET /` and `GET /{namespace}`.

use indexmap::IndexMap;
use speedyspec_operation_contracts::{OperationCatalog, SchemaKind, TypeCatalog, Value};

use crate::RouteTable;

fn type_name(kind: &SchemaKind) -> Option<&'static str> {
    Some(match kind {
        SchemaKind::Boolean => "boolean",
        SchemaKind::Integer { .. } => "integer",
        SchemaKind::Number { .. } => "number",
        SchemaKind::String { .. } | SchemaKind::DateTime | SchemaKind::Binary { .. } => "string",
        SchemaKind::Array { .. } => "array",
        SchemaKind::Object { .. } | SchemaKind::Ref { .. } => "object",
        SchemaKind::Any | SchemaKind::OneOf { .. } => return None,
    })
}

fn args(operation: &str, operations: &dyn OperationCatalog, types: &dyn TypeCatalog) -> Value {
    let Some(spec) = operations.get_operation(operation) else {
        return Value::Map(IndexMap::new());
    };
    let properties = match &spec.input.schema.kind {
        SchemaKind::Object { properties, .. } => properties.clone(),
        SchemaKind::Ref { type_name } => types
            .get_type(type_name)
            .map(|t| t.fields.clone())
            .unwrap_or_default(),
        _ => IndexMap::new(),
    };
    Value::Map(
        properties
            .iter()
            .map(|(name, p)| {
                let mut arg = Vec::new();
                if let Some(t) = type_name(&p.schema.kind) {
                    arg.push(("type", Value::from(t)));
                }
                arg.push(("required", Value::Bool(p.required)));
                (name.clone(), Value::map(arg))
            })
            .collect(),
    )
}

/// The namespaces the table serves, in order.
pub fn namespaces(table: &RouteTable) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for entry in table.routes() {
        if !out.contains(&entry.namespace) {
            out.push(entry.namespace);
        }
    }
    out
}

fn routes(
    table: &RouteTable,
    namespace: Option<&str>,
    operations: &dyn OperationCatalog,
    types: &dyn TypeCatalog,
) -> Value {
    let mut out: IndexMap<String, Value> = IndexMap::new();
    for entry in table.routes() {
        if namespace.is_some_and(|ns| ns != entry.namespace) {
            continue;
        }
        let methods = Value::List(
            entry
                .methods
                .iter()
                .map(|m| Value::from(m.as_str()))
                .collect(),
        );
        let endpoint = Value::map([
            ("methods", methods.clone()),
            ("args", args(&entry.operation, operations, types)),
        ]);
        let slot = out.entry(entry.route.clone()).or_insert_with(|| {
            Value::map([
                ("namespace", Value::from(entry.namespace.as_str())),
                ("methods", Value::List(Vec::new())),
                ("endpoints", Value::List(Vec::new())),
            ])
        });
        if let Value::Map(route) = slot {
            if let Some(Value::List(all)) = route.get_mut("methods") {
                for m in &entry.methods {
                    let m = Value::from(m.as_str());
                    if !all.contains(&m) {
                        all.push(m);
                    }
                }
            }
            if let Some(Value::List(endpoints)) = route.get_mut("endpoints") {
                endpoints.push(endpoint);
            }
        }
    }
    Value::Map(out)
}

/// `GET /`: every namespace and route.
pub fn root(
    table: &RouteTable,
    operations: &dyn OperationCatalog,
    types: &dyn TypeCatalog,
) -> Value {
    Value::map([
        (
            "namespaces",
            Value::List(
                namespaces(table)
                    .iter()
                    .map(|n| Value::from(n.as_str()))
                    .collect(),
            ),
        ),
        ("routes", routes(table, None, operations, types)),
    ])
}

/// `GET /{namespace}`: one namespace's routes.
pub fn namespace(
    table: &RouteTable,
    namespace: &str,
    operations: &dyn OperationCatalog,
    types: &dyn TypeCatalog,
) -> Value {
    Value::map([
        ("namespace", Value::from(namespace)),
        ("routes", routes(table, Some(namespace), operations, types)),
    ])
}
