//! The default REST mapping (Strategy): an operation's route from its name and resource traits.

use speedyspec_operation_contracts::{
    Action, Kind, OperationSpec, Property, SchemaKind, TypeCatalog,
};
use speedyspec_rest_contracts::{RestBinding, RestMethod};

/// Routes operations by their resource and action, under `{vendor}/v1`.
///
/// | Operation | Route | Methods |
/// |---|---|---|
/// | list | `/{plural}` | GET |
/// | get | `/{plural}/{identity}` | GET |
/// | create | `/{plural}` | POST |
/// | update | `/{plural}/{identity}` | POST, PUT, PATCH |
/// | delete | `/{plural}/{identity}` | DELETE |
/// | custom action | `/{plural}/{identity}/{action}` | GET (query) or POST (command) |
/// | no resource | `/{name after the vendor}` | GET (query) or POST (command) |
///
/// Subscriptions have no default route.
pub struct DefaultRestMapping;

/// The English plural of a resource type name.
pub fn plural(name: &str) -> String {
    let consonant_y = name.ends_with('y')
        && name
            .chars()
            .rev()
            .nth(1)
            .is_some_and(|c| !"aeiou".contains(c));
    if consonant_y {
        format!("{}ies", &name[..name.len() - 1])
    } else if ["s", "x", "ch", "sh"].iter().any(|end| name.ends_with(end)) {
        format!("{name}es")
    } else {
        format!("{name}s")
    }
}

fn input_properties<'a>(
    spec: &'a OperationSpec,
    types: &'a dyn TypeCatalog,
) -> Option<indexmap::IndexMap<String, Property>> {
    match &spec.input.schema.kind {
        SchemaKind::Object { properties, .. } => Some(properties.clone()),
        SchemaKind::Ref { type_name } => types.get_type(type_name).map(|t| t.fields.clone()),
        _ => None,
    }
}

impl DefaultRestMapping {
    pub fn binding(&self, spec: &OperationSpec, types: &dyn TypeCatalog) -> Option<RestBinding> {
        if spec.kind == Kind::Subscription {
            return None;
        }
        let (vendor, rest) = spec.name.as_str().split_once('/')?;
        let namespace = format!("{vendor}/v1");
        let by_kind = if spec.kind == Kind::Query {
            RestMethod::Get
        } else {
            RestMethod::Post
        };
        let Some(resource) = &spec.resource else {
            return Some(RestBinding {
                namespace,
                route: format!("/{rest}"),
                methods: vec![by_kind],
            });
        };
        let properties = input_properties(spec, types);
        let identity: String = resource
            .identity
            .iter()
            .map(|field| {
                let integer = properties
                    .as_ref()
                    .and_then(|p| p.get(field))
                    .is_some_and(|p| matches!(p.schema.kind, SchemaKind::Integer { .. }));
                if integer {
                    format!(r"/(?P<{field}>[\d]+)")
                } else {
                    format!("/(?P<{field}>[^/]+)")
                }
            })
            .collect();
        let base = format!("/{}", plural(&resource.type_name));
        let (route, methods) = match &resource.action {
            Action::List => (base, vec![RestMethod::Get]),
            Action::Get => (format!("{base}{identity}"), vec![RestMethod::Get]),
            Action::Create => (base, vec![RestMethod::Post]),
            Action::Update => (
                format!("{base}{identity}"),
                vec![RestMethod::Post, RestMethod::Put, RestMethod::Patch],
            ),
            Action::Delete => (format!("{base}{identity}"), vec![RestMethod::Delete]),
            Action::Custom(action) => (format!("{base}{identity}/{action}"), vec![by_kind]),
        };
        Some(RestBinding {
            namespace,
            route,
            methods,
        })
    }
}
