//! The REST transport.
//!
//! - [`DefaultRestMapping`] routes operations by resource and action (Strategy); [`RestTransport`] is the `rest`
//!   transport over it.
//! - [`RouteTable`] holds every REST route and matches requests as WordPress does.
//! - [`NoRestHooks`] is the hooks port's do-nothing default.

mod hooks;
mod mapping;
mod routes;
mod transport;

pub use hooks::NoRestHooks;
pub use mapping::{DefaultRestMapping, plural};
pub use routes::{RouteEntry, RouteMatch, RouteTable};
pub use transport::{REST, RestTransport, route_parameters};
