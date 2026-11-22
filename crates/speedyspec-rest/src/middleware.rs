//! The REST server's middleware: `rest_dispatch_request` inside the dispatcher, and the HTTP pipeline adapter.

use std::any::Any;
use std::cell::RefCell;
use std::rc::Rc;

use speedyspec_http::{Handler, Middleware};
use speedyspec_http_message::{HttpResponse, HttpServerRequest};
use speedyspec_operation_contracts::OperationError;
use speedyspec_operation_dispatch::{Bypass, Call, OperationMiddleware};
use speedyspec_rest_contracts::{RestHooks, RestResponse, SharedRequest};

use crate::RestServer;
use crate::transport::REST;

struct Frame {
    request: SharedRequest,
    answered: Option<RestResponse>,
}

thread_local! {
    /// The requests being dispatched, innermost last (WordPress's `dispatching_requests`).
    static DISPATCHING: RefCell<Vec<Frame>> = const { RefCell::new(Vec::new()) };
}

/// Marks `request` as being dispatched until the guard drops (also on error or panic).
pub(crate) struct Dispatching;

impl Dispatching {
    pub(crate) fn enter(request: SharedRequest) -> Dispatching {
        DISPATCHING.with(|d| {
            d.borrow_mut().push(Frame {
                request,
                answered: None,
            })
        });
        Dispatching
    }

    /// The response `rest_dispatch_request` answered with for this request, if it did.
    pub(crate) fn answered(&self) -> Option<RestResponse> {
        DISPATCHING.with(|d| d.borrow_mut().last_mut().and_then(|f| f.answered.take()))
    }
}

impl Drop for Dispatching {
    fn drop(&mut self) {
        DISPATCHING.with(|d| d.borrow_mut().pop());
    }
}

fn current() -> Option<SharedRequest> {
    DISPATCHING.with(|d| d.borrow().last().map(|f| f.request.clone()))
}

/// Fires `rest_dispatch_request` after the permission check, for calls through `rest`: an answer replaces the handler.
pub(crate) struct DispatchRequestMiddleware {
    pub hooks: Rc<dyn RestHooks>,
}

impl OperationMiddleware for DispatchRequestMiddleware {
    fn around(
        &self,
        call: &Call<'_>,
        next: &dyn Fn() -> Result<Box<dyn Any>, OperationError>,
    ) -> Result<Box<dyn Any>, OperationError> {
        if call.caller.transport != REST {
            return next();
        }
        let Some(request) = current() else {
            return next();
        };
        match self.hooks.dispatch_request(&request) {
            None => next(),
            Some(Err(error)) => Err(error),
            Some(Ok(response)) => {
                let data = response.data.clone();
                DISPATCHING.with(|d| {
                    if let Some(frame) = d.borrow_mut().last_mut() {
                        frame.answered = Some(response);
                    }
                });
                Ok(Box::new(Bypass(data)))
            }
        }
    }
}

/// The REST server in the HTTP pipeline: answers REST requests, passes everything else on.
pub struct RestMiddleware {
    server: Rc<RestServer>,
}

impl RestMiddleware {
    pub fn new(server: Rc<RestServer>) -> RestMiddleware {
        RestMiddleware { server }
    }
}

impl Middleware<HttpServerRequest, HttpResponse> for RestMiddleware {
    fn process(
        &self,
        request: HttpServerRequest,
        next: &dyn Handler<HttpServerRequest, HttpResponse>,
    ) -> HttpResponse {
        match self.server.serve(&request) {
            Some(response) => response,
            None => next.handle(request),
        }
    }
}
