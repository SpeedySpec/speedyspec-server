//! The `rest` transport.

use std::rc::Rc;
use std::sync::LazyLock;

use regex::Regex;
use speedyspec_operation_contracts::{OperationSpec, SchemaKind, TypeCatalog, Value};
use speedyspec_rest_contracts::RestBinding;
use speedyspec_transport_contracts::{Transport, TransportName};

use crate::mapping::DefaultRestMapping;

/// The transport's name.
pub const REST: &str = "rest";

static NAMED_GROUP: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\(\?P?<([A-Za-z_][A-Za-z0-9_]*)>").expect("a valid pattern"));

/// The named groups (URL parameters) of a route pattern.
pub fn route_parameters(route: &str) -> Vec<String> {
    NAMED_GROUP
        .captures_iter(route)
        .map(|c| c[1].to_owned())
        .collect()
}

/// WordPress's REST API as a transport: default routes from [`DefaultRestMapping`], explicit ones from `Rest::bind`.
pub struct RestTransport {
    name: TransportName,
    types: Rc<dyn TypeCatalog>,
}

impl RestTransport {
    pub fn new(types: Rc<dyn TypeCatalog>) -> RestTransport {
        RestTransport {
            name: TransportName::parse(REST).expect("rest is a transport name"),
            types,
        }
    }
}

impl Transport for RestTransport {
    fn name(&self) -> &TransportName {
        &self.name
    }

    fn default_binding(&self, spec: &OperationSpec) -> Option<Value> {
        DefaultRestMapping
            .binding(spec, self.types.as_ref())
            .map(|b| b.to_value())
    }

    fn check_binding(&self, spec: &OperationSpec, binding: &Value) -> Result<(), String> {
        let binding = RestBinding::from_value(binding)?;
        let inputs: Vec<String> = match &spec.input.schema.kind {
            SchemaKind::Object { properties, .. } => properties.keys().cloned().collect(),
            SchemaKind::Ref { type_name } => self
                .types
                .get_type(type_name)
                .map(|t| t.fields.keys().cloned().collect())
                .unwrap_or_default(),
            _ => Vec::new(),
        };
        if let Some(missing) = route_parameters(&binding.route)
            .into_iter()
            .find(|p| !inputs.contains(p))
        {
            return Err(format!(
                "route parameter {missing} is not an input of {}",
                spec.name.as_str()
            ));
        }
        Regex::new(&format!("(?i)^{}$", binding.full_route()))
            .map(drop)
            .map_err(|e| format!("route is not a valid pattern: {e}"))
    }
}
