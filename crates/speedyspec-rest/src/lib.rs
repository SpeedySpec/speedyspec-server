//! The REST transport.
//!
//! - [`DefaultRestMapping`] routes operations by resource and action (Strategy); [`RestTransport`] is the `rest`
//!   transport over it.
//! - [`RouteTable`] holds every REST route and matches requests as WordPress does.
//! - [`decode`] reads HTTP requests as REST requests ([`query`] with PHP's `parse_str` rules); [`coerce`] turns wire
//!   strings into the kinds their schema expects.
//! - [`errors`] maps operation errors to WordPress's statuses and bodies; [`encode`] writes HTTP responses; [`fields`]
//!   applies `_fields`.
//! - [`links`] builds `_links` and `_embed` from type relations.
//! - [`RestServer`] serves REST requests in WordPress trunk's hook order; [`RestMiddleware`] puts it in the HTTP
//!   pipeline; [`index`] answers `GET /` and `GET /{namespace}`.
//! - [`NoRestHooks`] is the hooks port's do-nothing default.

pub mod coerce;
pub mod decode;
pub mod encode;
pub mod errors;
pub mod fields;
mod hooks;
pub mod index;
pub mod links;
mod mapping;
mod middleware;
pub mod query;
mod routes;
mod server;
mod transport;

pub use hooks::NoRestHooks;
pub use mapping::{DefaultRestMapping, plural};
pub use middleware::RestMiddleware;
pub use routes::{RouteEntry, RouteMatch, RouteTable};
pub use server::RestServer;
pub use transport::{REST, RestTransport, route_parameters};
