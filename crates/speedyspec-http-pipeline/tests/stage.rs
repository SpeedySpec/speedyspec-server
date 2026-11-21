use std::rc::Rc;

use speedyspec_http::{
    Handler, Message, Method, Middleware, Response, ServerRequestFactory, StatusCode,
};
use speedyspec_http_message::{HttpResponse, HttpServerRequest, MessageFactory};
use speedyspec_http_pipeline::StageMiddleware;
use speedyspec_native_stage::{NativeResponse, NativeStage, StageOutcome, StageRequest};

struct Blocker {
    block: bool,
}

impl NativeStage for Blocker {
    fn name(&self) -> &str {
        "blocker"
    }

    fn answer(&self, request: &StageRequest<'_>) -> StageOutcome {
        if self.block && request.param("REMOTE_ADDR") == Some("203.0.113.7") {
            StageOutcome::Respond(NativeResponse {
                status: 403,
                headers: vec![("Cache-Control".into(), "no-store".into())],
                body: b"Forbidden\n".to_vec(),
            })
        } else {
            StageOutcome::Continue
        }
    }
}

struct Php;

impl Handler<HttpServerRequest, HttpResponse> for Php {
    fn handle(&self, _request: HttpServerRequest) -> HttpResponse {
        HttpResponse::new(StatusCode::new(200).unwrap())
    }
}

fn request() -> HttpServerRequest {
    MessageFactory
        .create_server_request(
            Method::Get,
            "http://example.com/",
            vec![("REMOTE_ADDR".into(), "203.0.113.7".into())],
        )
        .unwrap()
}

#[test]
fn stage_middleware_answers() {
    let middleware = StageMiddleware::new(Rc::new(Blocker { block: true }));
    let response = middleware.process(request(), &Php);
    assert_eq!(response.status().as_u16(), 403);
    assert_eq!(
        response.headers().get("cache-control").as_deref(),
        Some("no-store")
    );
    assert_eq!(
        response.body().borrow_mut().read_all().unwrap(),
        b"Forbidden\n"
    );
}

#[test]
fn stage_middleware_continues() {
    let middleware = StageMiddleware::new(Rc::new(Blocker { block: false }));
    assert_eq!(middleware.process(request(), &Php).status().as_u16(), 200);
}
