//! The `abilities` transport.

use std::rc::Rc;

use speedyspec_abilities_contracts::{AbilityBinding, AbilityMapping, is_ability_name};
use speedyspec_operation_contracts::{OperationSpec, Value};
use speedyspec_transport_contracts::{Transport, TransportName};

use crate::DefaultAbilityMapping;

/// The transport's name.
pub const ABILITIES: &str = "abilities";

/// WordPress's Abilities API as a transport: default metadata from [`DefaultAbilityMapping`], explicit metadata from
/// `Abilities::publish`.
pub struct AbilitiesTransport {
    name: TransportName,
    mapping: Rc<dyn AbilityMapping>,
}

impl AbilitiesTransport {
    /// The transport with [`DefaultAbilityMapping`].
    pub fn new() -> AbilitiesTransport {
        AbilitiesTransport::with_mapping(Rc::new(DefaultAbilityMapping))
    }

    /// The transport with another default mapping.
    pub fn with_mapping(mapping: Rc<dyn AbilityMapping>) -> AbilitiesTransport {
        AbilitiesTransport {
            name: TransportName::parse(ABILITIES).expect("abilities is a transport name"),
            mapping,
        }
    }

    /// The mapping explicit bindings are merged over.
    pub fn mapping(&self) -> &Rc<dyn AbilityMapping> {
        &self.mapping
    }
}

impl Default for AbilitiesTransport {
    fn default() -> Self {
        AbilitiesTransport::new()
    }
}

impl Transport for AbilitiesTransport {
    fn name(&self) -> &TransportName {
        &self.name
    }

    fn default_binding(&self, spec: &OperationSpec) -> Option<Value> {
        self.mapping.binding(spec).map(|b| b.to_value())
    }

    fn check_binding(&self, spec: &OperationSpec, binding: &Value) -> Result<(), String> {
        if !is_ability_name(spec.name.as_str()) {
            return Err(format!(
                "{} is not an ability name: namespace/name, lowercase letters, digits and dashes",
                spec.name.as_str()
            ));
        }
        AbilityBinding::from_value(binding).map(drop)
    }
}
