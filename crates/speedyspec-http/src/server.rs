/// Answers a request (PSR-15's `RequestHandlerInterface`).
///
/// The request and response types are parameters, not associated types, so a handler can be held as
/// `dyn Handler<Req, Res>` and middleware can call the next step without knowing what it is.
pub trait Handler<Req, Res> {
    fn handle(&self, request: Req) -> Res;
}

/// A step that may answer a request or pass it on (PSR-15's `MiddlewareInterface`).
pub trait Middleware<Req, Res> {
    fn process(&self, request: Req, next: &dyn Handler<Req, Res>) -> Res;
}
