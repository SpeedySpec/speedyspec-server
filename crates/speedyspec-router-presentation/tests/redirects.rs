use std::rc::Rc;

use speedyspec_http::{Method, ServerRequestFactory};
use speedyspec_http_message::{HttpServerRequest, MessageFactory};
use speedyspec_router_presentation::{RedirectSource, RedirectSources};

struct Fixed(&'static str, Option<(&'static str, u16)>);

impl RedirectSource for Fixed {
    fn redirect_for(&self, request: &HttpServerRequest) -> Option<(String, u16)> {
        use speedyspec_http::{Request, Uri};
        (request.uri().path() == self.0)
            .then(|| self.1.map(|(l, s)| (l.to_string(), s)))
            .flatten()
    }
}

#[test]
fn first_redirect_source_wins() {
    let sources = RedirectSources::new(vec![
        Rc::new(Fixed("/a", None)),
        Rc::new(Fixed("/a", Some(("/first", 301)))),
        Rc::new(Fixed("/a", Some(("/second", 302)))),
    ]);
    let request = MessageFactory
        .create_server_request(Method::Get, "http://example.test/a", vec![])
        .unwrap();
    assert_eq!(
        sources.redirect_for(&request),
        Some(("/first".to_string(), 301))
    );
    let other = MessageFactory
        .create_server_request(Method::Get, "http://example.test/b", vec![])
        .unwrap();
    assert_eq!(sources.redirect_for(&other), None);
}
