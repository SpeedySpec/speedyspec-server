//! The Abilities transport for the request: the composition root of the `WP_Ability` shim and, later, other callers.

use std::rc::Rc;

use speedyspec_abilities::{ABILITIES, AbilitiesServer, AbilitiesTransport, OperationSteps};
use speedyspec_abilities_contracts::AbilityBinding;
use speedyspec_container::Container;
use speedyspec_operation_contracts::{Caller, Identity};
use speedyspec_operation_contracts::{
    Category, OperationCatalog, OperationError, OperationSpec, Value,
};
use speedyspec_operation_dispatch::Dispatcher;
use speedyspec_operation_php::OperationArea;
use speedyspec_php_ext::Value as PhpValue;
use speedyspec_transport::TransportRegistry;
use speedyspec_transport_contracts::BindingStore;
use speedyspec_transport_contracts::{BindingSource, ExposureLookup};

use crate::PhpAbilityHooks;

/// Builds what executing an ability needs: the server with WordPress's hooks, the container legacy abilities' handlers
/// are bound in, and the caller (WordPress's current user).
pub struct AbilitiesService {
    container: Rc<Container>,
    operations: Rc<OperationArea>,
    bindings: Rc<dyn BindingStore>,
    identity: Rc<dyn Identity>,
}

impl AbilitiesService {
    pub fn new(
        container: Rc<Container>,
        operations: Rc<OperationArea>,
        bindings: Rc<dyn BindingStore>,
        identity: Rc<dyn Identity>,
    ) -> AbilitiesService {
        AbilitiesService {
            container,
            operations,
            bindings,
            identity,
        }
    }

    /// The container handlers are bound in.
    pub fn container(&self) -> &Rc<Container> {
        &self.container
    }

    /// The operation area the site's operations come from.
    pub fn operations(&self) -> &Rc<OperationArea> {
        &self.operations
    }

    /// Where `Abilities::publish` writes.
    pub fn bindings(&self) -> &Rc<dyn BindingStore> {
        &self.bindings
    }

    /// The server, with WordPress's ability hooks.
    pub fn server(&self) -> AbilitiesServer {
        AbilitiesServer::new(Rc::new(PhpAbilityHooks::new()))
    }

    /// The site's operations exposed as abilities, each with its metadata (explicit over the defaults) and whether
    /// its category is the default one, which the adapter registers when it is missing.
    pub fn abilities(
        &self,
    ) -> Result<Vec<(Rc<OperationSpec>, AbilityBinding, bool)>, OperationError> {
        let site = self.operations.site()?;
        let transport = Rc::new(AbilitiesTransport::new());
        let mapping = transport.mapping().clone();
        let transports = TransportRegistry::new(self.bindings.clone());
        let unavailable = |e: speedyspec_transport_contracts::ExposureError| {
            OperationError::new(
                "abilities_unavailable",
                Category::Unavailable,
                e.to_string(),
            )
        };
        transports.install(transport).map_err(unavailable)?;
        let exposure = transports.expose(&site.registries).map_err(unavailable)?;
        for problem in exposure.problems() {
            eprintln!("speedyspec_wordpress: an ability binding was skipped: {problem}");
        }
        let mut out = Vec::new();
        for exposed in exposure.exposed(ABILITIES) {
            let Some(defaults) = mapping.binding(&exposed.spec) else {
                continue;
            };
            let binding = match exposed.source {
                BindingSource::Default => defaults,
                BindingSource::Explicit => match AbilityBinding::from_value(&exposed.binding) {
                    Ok(explicit) => {
                        let explicit_category = explicit.category.is_some();
                        out.push((
                            exposed.spec.clone(),
                            explicit.merge(defaults),
                            !explicit_category,
                        ));
                        continue;
                    }
                    Err(why) => {
                        eprintln!(
                            "speedyspec_wordpress: {} is not an ability: {why}",
                            exposed.spec.name.as_str()
                        );
                        continue;
                    }
                },
            };
            out.push((exposed.spec.clone(), binding, true));
        }
        Ok(out)
    }

    /// The steps of the operation `name` executed as the ability whose object is `handle`.
    pub fn operation_steps(
        &self,
        name: &str,
        handle: u64,
    ) -> Result<OperationSteps, OperationError> {
        let site = self.operations.site()?;
        let spec = site.registries.get_operation(name).ok_or_else(|| {
            OperationError::new(
                "operation_not_found",
                Category::NotFound,
                format!("no operation {name}"),
            )
        })?;
        let dispatcher = Rc::new(Dispatcher::new(
            self.container.clone(),
            site.registries.clone(),
        ));
        Ok(OperationSteps::new(
            dispatcher,
            spec,
            self.caller(),
            Rc::new(PhpAbilityHooks::new()),
            handle,
        ))
    }

    /// WordPress's current user (`get_current_user_id()`, 0 for nobody) as an abilities caller.
    pub fn caller(&self) -> Caller {
        let user = match speedyspec_php::call(&PhpValue::Str("get_current_user_id".into()), &[]) {
            Ok(PhpValue::Int(id)) if id > 0 => u64::try_from(id).ok(),
            _ => None,
        };
        Caller::new(user, speedyspec_abilities::ABILITIES, self.identity.clone())
    }
}

/// A JSON value (an exported schema) as a neutral value.
pub(crate) fn from_json(json: &serde_json::Value) -> Value {
    match json {
        serde_json::Value::Null => Value::Null,
        serde_json::Value::Bool(b) => Value::Bool(*b),
        serde_json::Value::Number(n) => n
            .as_i64()
            .map(Value::Int)
            .unwrap_or_else(|| Value::Float(n.as_f64().unwrap_or(0.0))),
        serde_json::Value::String(s) => Value::String(s.clone()),
        serde_json::Value::Array(items) => Value::List(items.iter().map(from_json).collect()),
        serde_json::Value::Object(map) => {
            Value::Map(map.iter().map(|(k, v)| (k.clone(), from_json(v))).collect())
        }
    }
}
