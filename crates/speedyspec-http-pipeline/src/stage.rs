use std::rc::Rc;

use speedyspec_http::{Handler, Message, Middleware, Response, ServerRequest, StatusCode};
use speedyspec_http_message::{BytesBody, HttpResponse, HttpServerRequest};
use speedyspec_native_stage::{NativeStage, StageOutcome, StageRequest};

/// A native request stage as middleware, so the stage's crate needs no change to join the pipeline.
pub struct StageMiddleware {
    stage: Rc<dyn NativeStage>,
}

impl StageMiddleware {
    pub fn new(stage: Rc<dyn NativeStage>) -> StageMiddleware {
        StageMiddleware { stage }
    }
}

impl Middleware<HttpServerRequest, HttpResponse> for StageMiddleware {
    fn process(
        &self,
        request: HttpServerRequest,
        next: &dyn Handler<HttpServerRequest, HttpResponse>,
    ) -> HttpResponse {
        let outcome = self.stage.answer(&StageRequest {
            params: request.server_params(),
        });
        let StageOutcome::Respond(native) = outcome else {
            return next.handle(request);
        };
        let status = StatusCode::new(native.status)
            .unwrap_or_else(|_| StatusCode::new(500).expect("500 is valid"));
        let mut response = HttpResponse::new(status);
        for (name, value) in native.headers {
            response = match response.clone().with_added_header(&name, vec![value]) {
                Ok(with_header) => with_header,
                Err(err) => {
                    eprintln!(
                        "speedyspec_http_pipeline: stage {} sent a bad header: {err}",
                        self.stage.name()
                    );
                    response
                }
            };
        }
        response
            .with_body(Box::new(BytesBody::new(native.body)))
            .with_status(status, "")
    }
}
