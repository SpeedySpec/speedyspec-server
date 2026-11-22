//! The REST transport's contracts.
//!
//! [`RestBinding`] says where an operation is served; [`RestRequest`] and [`RestResponse`] are what hooks and handlers
//! see (WordPress's `WP_REST_Request` and `WP_REST_Response`); [`RestHooks`] is the port for WordPress's REST hooks.
//! Depends only on `speedyspec-operation-contracts`. Implementations: `speedyspec-rest` (and its `NoRestHooks`),
//! `speedyspec-rest-php` (the PHP hooks adapter).

mod binding;
mod hooks;
mod message;

pub use binding::{RestBinding, RestMethod};
pub use hooks::{RestHooks, RestOutcome};
pub use message::{RestLink, RestRequest, RestResponse, SharedRequest, header_key};
