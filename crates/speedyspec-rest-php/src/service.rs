//! One REST server per site: the composition root PHP's `rest_do_request` and, later, the server's HTTP wiring use.

use std::cell::RefCell;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::rc::Rc;

use speedyspec_container::Container;
use speedyspec_operation::Registries;
use speedyspec_operation_contracts::{Category, Identity, OperationError};
use speedyspec_operation_php::OperationArea;
use speedyspec_rest::{RestServer, RestTransport};
use speedyspec_transport::TransportRegistry;
use speedyspec_transport_contracts::BindingStore;

use crate::{PhpRestAuthenticator, PhpRestHooks};

struct Built {
    registries: Rc<Registries>,
    bindings: u64,
    server: Rc<RestServer>,
}

/// Builds and keeps the REST server of the site the current request serves, with the PHP hooks and authenticator. The
/// server is rebuilt when the site's registries are new (a new site, or plugins or theme changed) or the REST bindings
/// changed.
pub struct RestService {
    container: Rc<Container>,
    operations: Rc<OperationArea>,
    bindings: Rc<dyn BindingStore>,
    identity: Rc<dyn Identity>,
    base_url: Rc<dyn Fn() -> String>,
    built: RefCell<Option<Built>>,
}

impl RestService {
    pub fn new(
        container: Rc<Container>,
        operations: Rc<OperationArea>,
        bindings: Rc<dyn BindingStore>,
        identity: Rc<dyn Identity>,
        base_url: Rc<dyn Fn() -> String>,
    ) -> RestService {
        RestService {
            container,
            operations,
            bindings,
            identity,
            base_url,
            built: RefCell::new(None),
        }
    }

    /// The container legacy handlers are bound in.
    pub fn container(&self) -> &Rc<Container> {
        &self.container
    }

    /// The operation area legacy routes register into.
    pub fn operations(&self) -> &Rc<OperationArea> {
        &self.operations
    }

    /// Where `Rest::bind` and legacy registrations write their bindings.
    pub fn bindings(&self) -> &Rc<dyn BindingStore> {
        &self.bindings
    }

    fn digest(&self) -> u64 {
        let mut hasher = DefaultHasher::new();
        format!("{:?}", self.bindings.bindings()).hash(&mut hasher);
        hasher.finish()
    }

    /// The current site's REST server, built on first use and whenever the site or the bindings changed.
    pub fn server(&self) -> Result<Rc<RestServer>, OperationError> {
        crate::area::ensure_legacy_hook();
        let site = self.operations.site()?;
        let digest = self.digest();
        if let Some(built) = self.built.borrow().as_ref()
            && Rc::ptr_eq(&built.registries, &site.registries)
            && built.bindings == digest
        {
            return Ok(built.server.clone());
        }
        let transports = TransportRegistry::new(self.bindings.clone());
        transports
            .install(Rc::new(RestTransport::new(site.registries.clone())))
            .map_err(|e| {
                OperationError::new("rest_unavailable", Category::Internal, e.to_string())
            })?;
        let exposure = transports.expose(&site.registries).map_err(|e| {
            OperationError::new("rest_unavailable", Category::Unavailable, e.to_string())
        })?;
        for problem in exposure.problems() {
            eprintln!("speedyspec_wordpress: a REST binding was skipped: {problem}");
        }
        let server = Rc::new(RestServer::new(
            self.container.clone(),
            site.registries.clone(),
            Rc::new(exposure),
            Rc::new(PhpRestHooks::new()),
            Rc::new(PhpRestAuthenticator::new(self.identity.clone())),
            &(self.base_url)(),
        ));
        for problem in server.problems() {
            eprintln!("speedyspec_wordpress: a REST route was skipped: {problem}");
        }
        *self.built.borrow_mut() = Some(Built {
            registries: site.registries.clone(),
            bindings: digest,
            server: server.clone(),
        });
        Ok(server)
    }
}
