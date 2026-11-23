//! The default XML-RPC mapping (Strategy): an operation's method name and parameters from its name and resource.

use indexmap::IndexMap;
use speedyspec_operation_contracts::{
    Action, Kind, OperationSpec, Property, SchemaKind, TypeCatalog,
};
use speedyspec_transport::naming::{pascal, plural};
use speedyspec_xmlrpc_contracts::{XmlRpcBinding, XmlRpcMapping, XmlRpcParam};

/// Names methods after WordPress's own (`wp.getPost`, `wp.newPost`, `wp.editPost`), under the operation's vendor.
///
/// | Operation | Method | Params |
/// |---|---|---|
/// | list | `{vendor}.get{Plural}` | `blog_id, username, password, *` |
/// | get | `{vendor}.get{Type}` | `blog_id, username, password, {identity…}, *` |
/// | create | `{vendor}.new{Type}` | `blog_id, username, password, *` |
/// | update | `{vendor}.edit{Type}` | `blog_id, username, password, {identity…}, *` |
/// | delete | `{vendor}.delete{Type}` | `blog_id, username, password, {identity…}` |
/// | custom action | `{vendor}.{action}{Type}` | `blog_id, username, password, {identity…}, *` |
/// | no resource | `{vendor}.{name after the vendor}` | `blog_id, username, password, *` |
///
/// Names are camelCase; a vendor's characters other than letters, digits and `_` become `_`. `*` is one struct of
/// the input properties left, and is left out when none are. Subscriptions have no method.
pub struct DefaultXmlRpcMapping;

/// An operation's input properties, from its object schema or the type it refers to.
pub(crate) fn input_properties(
    spec: &OperationSpec,
    types: &dyn TypeCatalog,
) -> Option<IndexMap<String, Property>> {
    match &spec.input.schema.kind {
        SchemaKind::Object { properties, .. } => Some(properties.clone()),
        SchemaKind::Ref { type_name } => types.get_type(type_name).map(|t| t.fields.clone()),
        _ => None,
    }
}

/// `name` in camelCase: `mark-read` is `markRead`.
fn camel(name: &str) -> String {
    let pascal = pascal(name);
    let mut chars = pascal.chars();
    chars
        .next()
        .map(|first| first.to_lowercase().chain(chars).collect())
        .unwrap_or_default()
}

impl XmlRpcMapping for DefaultXmlRpcMapping {
    fn binding(&self, spec: &OperationSpec, types: &dyn TypeCatalog) -> Option<XmlRpcBinding> {
        if spec.kind == Kind::Subscription {
            return None;
        }
        let (vendor, rest) = spec.name.as_str().split_once('/')?;
        let vendor: String = vendor
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
            .collect();
        let (verb, identity) = match &spec.resource {
            None => (camel(rest), Vec::new()),
            Some(resource) => {
                let type_name = pascal(&resource.type_name);
                let verb = match &resource.action {
                    Action::List => format!("get{}", pascal(&plural(&resource.type_name))),
                    Action::Get => format!("get{type_name}"),
                    Action::Create => format!("new{type_name}"),
                    Action::Update => format!("edit{type_name}"),
                    Action::Delete => format!("delete{type_name}"),
                    Action::Custom(action) => format!("{}{type_name}", camel(action)),
                };
                let identity = match resource.action {
                    Action::List | Action::Create => Vec::new(),
                    _ => resource.identity.clone(),
                };
                (verb, identity)
            }
        };
        let mut params = vec![
            XmlRpcParam::BlogId,
            XmlRpcParam::Username,
            XmlRpcParam::Password,
        ];
        params.extend(identity.iter().cloned().map(XmlRpcParam::Field));
        let properties = input_properties(spec, types).unwrap_or_default();
        if properties.keys().any(|key| !identity.contains(key)) {
            params.push(XmlRpcParam::Rest);
        }
        Some(XmlRpcBinding {
            method: format!("{vendor}.{verb}"),
            params,
        })
    }
}
