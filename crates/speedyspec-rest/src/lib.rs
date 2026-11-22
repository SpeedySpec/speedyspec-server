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
//! - [`NoRestHooks`] is the hooks port's do-nothing default.

pub mod coerce;
pub mod decode;
pub mod encode;
pub mod errors;
pub mod fields;
mod hooks;
pub mod links;
mod mapping;
pub mod query;
mod routes;
mod transport;

pub use hooks::NoRestHooks;
pub use mapping::{DefaultRestMapping, plural};
pub use routes::{RouteEntry, RouteMatch, RouteTable};
pub use transport::{REST, RestTransport, route_parameters};
