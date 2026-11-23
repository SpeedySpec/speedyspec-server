//! `xmlrpc.php` in the HTTP pipeline.

use std::rc::Rc;

use speedyspec_http::{Handler, Middleware, Request, Uri};
use speedyspec_http_message::{HttpResponse, HttpServerRequest};
use speedyspec_operation_contracts::OperationError;
use speedyspec_xmlrpc_contracts::{Fault, MethodResponse};

use crate::XmlRpcServer;
use crate::codec::write_response;
use crate::server::respond;

/// Gives the XML-RPC server of the request being served. A server lasts one request (its method table and its
/// failed-login latch are the request's), so the middleware asks for one on every request.
pub trait XmlRpcServerProvider {
    fn server(&self) -> Result<Rc<XmlRpcServer>, OperationError>;
}

/// Serves requests for `xmlrpc.php` (at the site root or under a subdirectory) and passes everything else on.
pub struct XmlRpcMiddleware {
    provider: Rc<dyn XmlRpcServerProvider>,
}

impl XmlRpcMiddleware {
    pub fn new(provider: Rc<dyn XmlRpcServerProvider>) -> XmlRpcMiddleware {
        XmlRpcMiddleware { provider }
    }
}

impl Middleware<HttpServerRequest, HttpResponse> for XmlRpcMiddleware {
    fn process(
        &self,
        request: HttpServerRequest,
        next: &dyn Handler<HttpServerRequest, HttpResponse>,
    ) -> HttpResponse {
        let path = request.uri().path();
        if path != "/xmlrpc.php" && !path.ends_with("/xmlrpc.php") {
            return next.handle(request);
        }
        match self.provider.server() {
            Ok(server) => server.serve(&request),
            Err(error) => {
                eprintln!("speedyspec_xmlrpc: the XML-RPC server is not available: {error}");
                let fault = Fault::new(500, "The request could not be processed.");
                respond(
                    200,
                    &[("Content-Type", "text/xml; charset=UTF-8")],
                    &write_response(&MethodResponse::Fault(fault)),
                )
            }
        }
    }
}
