//! The `xmlrpc` transport.

use std::rc::Rc;

use speedyspec_operation_contracts::{OperationSpec, SchemaKind, TypeCatalog, Value};
use speedyspec_transport_contracts::{Transport, TransportName};
use speedyspec_xmlrpc_contracts::{XmlRpcBinding, XmlRpcMapping, XmlRpcParam};

use crate::mapping::{DefaultXmlRpcMapping, input_properties};

/// The transport's name.
pub const XMLRPC: &str = "xmlrpc";

/// WordPress's XML-RPC API as a transport: default methods from [`DefaultXmlRpcMapping`], explicit ones from
/// `XmlRpc::bind`.
pub struct XmlRpcTransport {
    name: TransportName,
    types: Rc<dyn TypeCatalog>,
    mapping: Rc<dyn XmlRpcMapping>,
}

impl XmlRpcTransport {
    /// The transport with [`DefaultXmlRpcMapping`].
    pub fn new(types: Rc<dyn TypeCatalog>) -> XmlRpcTransport {
        XmlRpcTransport::with_mapping(types, Rc::new(DefaultXmlRpcMapping))
    }

    /// The transport with another default mapping.
    pub fn with_mapping(
        types: Rc<dyn TypeCatalog>,
        mapping: Rc<dyn XmlRpcMapping>,
    ) -> XmlRpcTransport {
        XmlRpcTransport {
            name: TransportName::parse(XMLRPC).expect("xmlrpc is a transport name"),
            types,
            mapping,
        }
    }
}

impl Transport for XmlRpcTransport {
    fn name(&self) -> &TransportName {
        &self.name
    }

    fn default_binding(&self, spec: &OperationSpec) -> Option<Value> {
        self.mapping
            .binding(spec, self.types.as_ref())
            .map(|b| b.to_value())
    }

    fn check_binding(&self, spec: &OperationSpec, binding: &Value) -> Result<(), String> {
        let binding = XmlRpcBinding::from_value(binding)?;
        let rests = binding
            .params
            .iter()
            .filter(|p| **p == XmlRpcParam::Rest)
            .count();
        if rests > 1 || (rests == 1 && binding.params.last() != Some(&XmlRpcParam::Rest)) {
            return Err("* may appear once, as the last parameter".into());
        }
        // An open input (`any`) takes any field, as legacy methods' inputs do.
        if matches!(spec.input.schema.kind, SchemaKind::Any) {
            return Ok(());
        }
        let properties = input_properties(spec, self.types.as_ref()).unwrap_or_default();
        for param in &binding.params {
            if let XmlRpcParam::Field(field) = param
                && !properties.contains_key(field)
            {
                return Err(format!(
                    "parameter {field} is not an input of {}",
                    spec.name.as_str()
                ));
            }
        }
        Ok(())
    }
}
