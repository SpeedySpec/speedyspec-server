//! The REST transport's PHP adapter.
//!
//! - [`PhpRestHooks`] fires WordPress's REST hooks with `apply_filters`. The request is a
//!   `SpeedySpec\Rest\RequestProxy` (a `WP_REST_Request`) and responses are `SpeedySpec\Rest\ResponseProxy` (a
//!   `WP_REST_Response`): proxies that read and write the Rust request and response live, so a hook's `set_param()`
//!   reaches the handler. The proxies are declared in a request once WordPress's classes exist to extend.
//! - [`errors`] maps `WP_Error` to and from operation errors by its `status`, and reads back what hooks return.
//! - [`PhpRestAuthenticator`] makes WordPress's current user the caller.
//! - [`RestArea`] gives PHP `SpeedySpec\Rest\Rest::bind( $operation )->route( $namespace, $route )->methods( … )`.
//!
//! Handles to Rust objects live for one request; a proxy used after its request throws `LogicException`.

mod area;
mod auth;
pub mod errors;
mod handles;
mod hooks;

pub use area::RestArea;
pub use auth::PhpRestAuthenticator;
pub use hooks::PhpRestHooks;
