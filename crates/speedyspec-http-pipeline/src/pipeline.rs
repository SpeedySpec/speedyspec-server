use std::rc::Rc;

use speedyspec_http::{Handler, Middleware};

/// Middleware in the order they were piped, then a final handler.
pub struct Pipeline<Req, Res> {
    middleware: Vec<Rc<dyn Middleware<Req, Res>>>,
    last: Rc<dyn Handler<Req, Res>>,
}

impl<Req, Res> Pipeline<Req, Res> {
    pub fn new(final_handler: Rc<dyn Handler<Req, Res>>) -> Pipeline<Req, Res> {
        Pipeline {
            middleware: Vec::new(),
            last: final_handler,
        }
    }

    /// Adds `middleware` after every one piped so far.
    pub fn pipe(&mut self, middleware: Rc<dyn Middleware<Req, Res>>) {
        self.middleware.push(middleware);
    }
}

/// The rest of the pipeline from one position, handed to the middleware at that position as its `next`.
struct Step<'p, Req, Res> {
    pipeline: &'p Pipeline<Req, Res>,
    at: usize,
}

impl<Req, Res> Handler<Req, Res> for Step<'_, Req, Res> {
    fn handle(&self, request: Req) -> Res {
        match self.pipeline.middleware.get(self.at) {
            Some(middleware) => middleware.process(
                request,
                &Step {
                    pipeline: self.pipeline,
                    at: self.at + 1,
                },
            ),
            None => self.pipeline.last.handle(request),
        }
    }
}

impl<Req, Res> Handler<Req, Res> for Pipeline<Req, Res> {
    fn handle(&self, request: Req) -> Res {
        Step {
            pipeline: self,
            at: 0,
        }
        .handle(request)
    }
}
