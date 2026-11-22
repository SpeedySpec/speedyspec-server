//! One REST server per site: the composition root PHP's `rest_do_request` and, later, the server's HTTP wiring use.

use std::cell::RefCell;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::rc::Rc;

use speedyspec_container::Container;
use speedyspec_operation::Registries;
use speedyspec_operation_contracts::{Category, Identity, OperationError};
use speedyspec_operation_contracts::{OperationCatalog, TypeCatalog};
use speedyspec_operation_php::OperationArea;
use speedyspec_php_ext::Value as PhpValue;
use speedyspec_rest::{RestServer, RestTransport};
use speedyspec_transport::{MemoryBindingStore, TransportRegistry};
use speedyspec_transport_contracts::BindingStore;

use crate::{PhpRestAuthenticator, PhpRestHooks};

struct Built {
    site: Rc<Registries>,
    bindings: u64,
    legacy: String,
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

    /// The current user as a REST caller.
    pub fn caller(&self) -> speedyspec_operation_contracts::Caller {
        PhpRestAuthenticator::new(self.identity.clone()).current()
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

    /// The current site's REST server, built on first use and whenever the site, the bindings or this request's
    /// legacy routes changed. It serves the site's operations plus the legacy routes.
    pub fn server(&self) -> Result<Rc<RestServer>, OperationError> {
        crate::area::ensure_legacy_hook();
        let site = self.operations.site()?;
        // wp_operations_init may have fired earlier in this request (an Operations::call) before the hook was added:
        // build WordPress's REST server now, so rest_api_init registers the legacy routes.
        if matches!(
            crate::errors::adapter("server", &[]),
            Some(PhpValue::Null) | None
        ) {
            crate::errors::adapter("rest_init", &[]);
        }
        crate::legacy::apply_endpoints_filter();
        let digest = self.digest();
        let legacy = crate::legacy::digest();
        if let Some(built) = self.built.borrow().as_ref()
            && Rc::ptr_eq(&built.site, &site.registries)
            && built.bindings == digest
            && built.legacy == legacy
        {
            return Ok(built.server.clone());
        }
        let (registries, bindings) = self.combine(&site.registries);
        let transports = TransportRegistry::new(bindings);
        transports
            .install(Rc::new(RestTransport::new(registries.clone())))
            .map_err(|e| {
                OperationError::new("rest_unavailable", Category::Internal, e.to_string())
            })?;
        let exposure = transports.expose(&registries).map_err(|e| {
            OperationError::new("rest_unavailable", Category::Unavailable, e.to_string())
        })?;
        for problem in exposure.problems() {
            eprintln!("speedyspec_wordpress: a REST binding was skipped: {problem}");
        }
        let server = Rc::new(RestServer::new(
            self.container.clone(),
            registries,
            Rc::new(exposure),
            Rc::new(PhpRestHooks::new()),
            Rc::new(PhpRestAuthenticator::new(self.identity.clone())),
            &(self.base_url)(),
        ));
        for problem in server.problems() {
            eprintln!("speedyspec_wordpress: a REST route was skipped: {problem}");
        }
        *self.built.borrow_mut() = Some(Built {
            site: site.registries.clone(),
            bindings: digest,
            legacy,
            server: server.clone(),
        });
        Ok(server)
    }

    /// The registries served (the site's types and operations, plus this request's legacy routes), closed, and the
    /// bindings (the stored ones, plus the legacy routes').
    fn combine(&self, site: &Registries) -> (Rc<Registries>, Rc<dyn BindingStore>) {
        let registries = Registries::new();
        for name in site.type_names() {
            if let Some(spec) = site.get_type(&name) {
                let _ = registries.register_type((*spec).clone());
            }
        }
        for name in site.operation_names() {
            if let Some(spec) = site.get_operation(&name) {
                let _ = registries.register_operation((*spec).clone());
            }
        }
        let bindings = MemoryBindingStore::new();
        for (operation, transport, config) in self.bindings.bindings() {
            bindings.bind(&operation, &transport, config);
        }
        for entry in crate::legacy::entries() {
            let name = entry.spec.name.as_str().to_owned();
            if let Err(err) = registries.register_operation(entry.spec) {
                eprintln!("speedyspec_wordpress: a legacy REST route was not served: {err}");
                continue;
            }
            bindings.bind(&name, speedyspec_rest::REST, entry.binding.to_value());
        }
        for problem in registries.close_lenient() {
            eprintln!("speedyspec_wordpress: a REST definition was dropped: {problem}");
        }
        (Rc::new(registries), Rc::new(bindings))
    }
}
