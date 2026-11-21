use std::rc::Rc;

use speedyspec_http_message::HttpServerRequest;

/// Something that may redirect a request before routing (the redirect-rules store, when it is built).
pub trait RedirectSource {
    /// The location and status to redirect to, or `None`.
    fn redirect_for(&self, request: &HttpServerRequest) -> Option<(String, u16)>;
}

/// Redirect sources asked in order; the first answer wins (Chain of Responsibility).
#[derive(Clone, Default)]
pub struct RedirectSources(Vec<Rc<dyn RedirectSource>>);

impl RedirectSources {
    pub fn new(sources: Vec<Rc<dyn RedirectSource>>) -> RedirectSources {
        RedirectSources(sources)
    }

    pub fn redirect_for(&self, request: &HttpServerRequest) -> Option<(String, u16)> {
        self.0
            .iter()
            .find_map(|source| source.redirect_for(request))
    }
}
