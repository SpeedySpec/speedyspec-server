//! The Abilities transport for the request: the composition root of the `WP_Ability` shim and, later, other callers.

use std::rc::Rc;

use speedyspec_abilities::AbilitiesServer;
use speedyspec_container::Container;
use speedyspec_operation_contracts::{Caller, Identity};
use speedyspec_operation_php::OperationArea;
use speedyspec_php_ext::Value as PhpValue;
use speedyspec_transport_contracts::BindingStore;

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

    /// WordPress's current user (`get_current_user_id()`, 0 for nobody) as an abilities caller.
    pub fn caller(&self) -> Caller {
        let user = match speedyspec_php::call(&PhpValue::Str("get_current_user_id".into()), &[]) {
            Ok(PhpValue::Int(id)) if id > 0 => u64::try_from(id).ok(),
            _ => None,
        };
        Caller::new(user, speedyspec_abilities::ABILITIES, self.identity.clone())
    }
}
