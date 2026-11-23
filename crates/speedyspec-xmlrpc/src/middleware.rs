//! `xmlrpc.php` in the HTTP pipeline.

use std::rc::Rc;

use speedyspec_http::{Handler, Middleware, Request, Uri};
use speedyspec_http_message::{HttpResponse, HttpServerRequest};

use crate::XmlRpcServer;

/// Serves requests for `xmlrpc.php` (at the site root or under a subdirectory) and passes everything else on.
pub struct XmlRpcMiddleware {
    server: Rc<XmlRpcServer>,
}

impl XmlRpcMiddleware {
    pub fn new(server: Rc<XmlRpcServer>) -> XmlRpcMiddleware {
        XmlRpcMiddleware { server }
    }
}

impl Middleware<HttpServerRequest, HttpResponse> for XmlRpcMiddleware {
    fn process(
        &self,
        request: HttpServerRequest,
        next: &dyn Handler<HttpServerRequest, HttpResponse>,
    ) -> HttpResponse {
        let path = request.uri().path();
        if path == "/xmlrpc.php" || path.ends_with("/xmlrpc.php") {
            self.server.serve(&request)
        } else {
            next.handle(request)
        }
    }
}
