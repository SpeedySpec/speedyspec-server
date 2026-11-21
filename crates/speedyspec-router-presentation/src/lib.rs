//! Routing as request stage 3, before PHP runs (eager mode).
//!
//! [`RoutingMiddleware`] resolves each request in the pipeline: the app's own routes answer natively, redirect sources
//! and redirect routes answer with a redirect (through WordPress's `wp_redirect` filters), and a request the server
//! sent to the app's entry script is matched against WordPress's rules, its resolution stored on the request for
//! PHP's `parse_request` to read. It is built and tested here and wired into the runtime by the runtime's owner,
//! once the runtime can read options before PHP and knows which hooks have PHP callbacks.

mod middleware;
mod redirects;
mod subject;

pub use middleware::{
    AppHandler, AppMiddleware, PhpCallbackRegistry, RoutingMiddleware, UnansweredPageGuard,
};
pub use redirects::{RedirectSource, RedirectSources};
pub use subject::wp_subject;
