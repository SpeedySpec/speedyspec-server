//! The XML-RPC server of a request: the composition root PHP's `wp_xmlrpc_server` and, later, the server's HTTP
//! wiring use.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use speedyspec_container::Container;
use speedyspec_http_message::{HttpResponse, HttpServerRequest};
use speedyspec_operation::Registries;
use speedyspec_operation_contracts::{
    Category, Identity, OperationCatalog, OperationError, TypeCatalog,
};
use speedyspec_operation_php::OperationArea;
use speedyspec_transport::{Exposure, TransportRegistry};
use speedyspec_transport_contracts::{BindingStore, ExposureLookup};
use speedyspec_xmlrpc::{
    DefaultFaultMapping, XMLRPC, XmlRpcServer, XmlRpcServerProvider, XmlRpcTransport,
};
use speedyspec_xmlrpc_contracts::XmlRpcBinding;

use crate::convert::adapter;
use crate::{PhpXmlRpcHooks, PhpXmlRpcLogin};

/// Builds the request's XML-RPC server: the site's operations exposed on `xmlrpc`, plus the legacy methods of the
/// request's `wp_xmlrpc_server`, with WordPress's hooks and login. A new `wp_xmlrpc_server` ([`XmlRpcService::reset`])
/// gets a new server for its table; the failed-login latch belongs to the request, shared by every server in it, and
/// is cleared only by [`XmlRpcService::end_request`].
pub struct XmlRpcService {
    container: Rc<Container>,
    operations: Rc<OperationArea>,
    bindings: Rc<dyn BindingStore>,
    identity: Rc<dyn Identity>,
    server: RefCell<Option<Rc<XmlRpcServer>>>,
    latch: Rc<Cell<bool>>,
}

fn unavailable(error: impl std::fmt::Display) -> OperationError {
    OperationError::new(
        "xmlrpc_unavailable",
        Category::Unavailable,
        error.to_string(),
    )
}

impl XmlRpcService {
    pub fn new(
        container: Rc<Container>,
        operations: Rc<OperationArea>,
        bindings: Rc<dyn BindingStore>,
        identity: Rc<dyn Identity>,
    ) -> XmlRpcService {
        XmlRpcService {
            container,
            operations,
            bindings,
            identity,
            server: RefCell::new(None),
            latch: Rc::new(Cell::new(false)),
        }
    }

    /// A new `wp_xmlrpc_server`: the next call builds a server for its table.
    pub fn reset(&self) {
        *self.server.borrow_mut() = None;
    }

    /// The request ended: its server, its legacy methods and its failed-login latch are forgotten.
    pub fn end_request(&self) {
        self.reset();
        self.latch.set(false);
        crate::legacy::end_request();
    }

    /// The site's operations exposed on `xmlrpc`.
    pub fn exposure(&self) -> Result<Rc<Exposure>, OperationError> {
        let site = self.operations.site()?;
        let transports = TransportRegistry::new(self.bindings.clone());
        transports
            .install(Rc::new(XmlRpcTransport::new(site.registries.clone())))
            .map_err(unavailable)?;
        let exposure = transports.expose(&site.registries).map_err(unavailable)?;
        for problem in exposure.problems() {
            eprintln!("speedyspec_wordpress: an XML-RPC binding was skipped: {problem}");
        }
        Ok(Rc::new(exposure))
    }

    /// The exposed operations' method names, for the method table: `(method, operation)`.
    pub fn operation_methods(&self) -> Result<Vec<(String, String)>, OperationError> {
        let exposure = self.exposure()?;
        Ok(exposure
            .exposed(XMLRPC)
            .iter()
            .filter_map(|exposed| {
                XmlRpcBinding::from_value(&exposed.binding)
                    .ok()
                    .map(|b| (b.method, exposed.spec.name.as_str().to_owned()))
            })
            .collect())
    }

    /// The request's server, built on first use: the request's `wp_xmlrpc_server` is created first (through
    /// `wp_xmlrpc_server_class`) when none exists, so its method table is the one served.
    pub fn server(&self) -> Result<Rc<XmlRpcServer>, OperationError> {
        if let Some(server) = self.server.borrow().as_ref() {
            return Ok(server.clone());
        }
        adapter("ensure_server", &[]);
        if let Some(server) = self.server.borrow().as_ref() {
            return Ok(server.clone());
        }
        for problem in crate::legacy::read_table(&self.container) {
            eprintln!("speedyspec_wordpress: an XML-RPC method was not served: {problem}");
        }
        let site = self.operations.site()?;
        let exposure = self.exposure()?;
        let server = Rc::new(
            XmlRpcServer::new(
                self.container.clone(),
                combine(&site.registries),
                exposure,
                Rc::new(PhpXmlRpcHooks::new()),
                Rc::new(PhpXmlRpcLogin::new(self.identity.clone())),
                Rc::new(DefaultFaultMapping),
                self.identity.clone(),
            )
            .with_login_latch(self.latch.clone()),
        );
        *self.server.borrow_mut() = Some(server.clone());
        Ok(server)
    }

    /// Answers one HTTP request to `xmlrpc.php`.
    pub fn serve(&self, request: &HttpServerRequest) -> Result<HttpResponse, OperationError> {
        Ok(self.server()?.serve(request))
    }
}

/// The site's types and operations plus this request's legacy methods, closed.
fn combine(site: &Registries) -> Rc<Registries> {
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
    for spec in crate::legacy::operations() {
        let name = spec.name.as_str().to_owned();
        if let Err(err) = registries.register_operation(spec) {
            eprintln!(
                "speedyspec_wordpress: the legacy XML-RPC method {name} was not served: {err}"
            );
        }
    }
    for problem in registries.close_lenient() {
        eprintln!("speedyspec_wordpress: an XML-RPC definition was dropped: {problem}");
    }
    Rc::new(registries)
}

impl XmlRpcServerProvider for XmlRpcService {
    fn server(&self) -> Result<Rc<XmlRpcServer>, OperationError> {
        XmlRpcService::server(self)
    }
}
