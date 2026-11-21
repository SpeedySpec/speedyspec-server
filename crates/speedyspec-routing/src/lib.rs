//! Routing contracts.
//!
//! What a route is, what resolving a request answers, the plain route data a table is compiled from, the store that
//! caches that data, the dispatch strategy interface, and the WordPress routing concepts the router supports
//! natively: ordered regex rules mapped to query variables, rewrite tags, permalink structures and endpoint masks.
//!
//! The router's implementation lives in `speedyspec-router`; this crate holds only types and traits, so an
//! implementation can be replaced partly or wholly.

pub mod conformance;
mod data;
mod dispatch;
mod guard;
mod resolution;
mod route;
mod store;
mod wordpress;

pub use data::{DynamicRoute, ROUTE_DATA_VERSION, RouteData, StaticRoute};
pub use dispatch::{CompiledDispatch, DispatchStrategy};
pub use guard::{GuardAnswer, RuleGuard};
pub use resolution::{QueryValue, QueryVars, Resolution, RouteMatch, RuleMatch};
pub use route::{Fingerprint, Params, RouteId, RouteName};
pub use store::{RouteTableStore, StoreError};
pub use wordpress::{
    Endpoint, EndpointMask, Permastruct, PermastructArgs, RewriteTag, Rule, RulePriority,
};
