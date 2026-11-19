//! The contract between `speedyspec-server` and `speedyspec-php-runtime`. It covers only how a worker is started
//! ([`WorkerSpec`]), the per-request report a worker sends after each request ([`WorkerReport`]), and what the
//! runtime binary says about itself ([`RuntimeManifest`]). Requests themselves are FastCGI 1.0.
//!
//! Adding a field is a minor change the other side ignores; anything else is a new [`CONTRACT_MAJOR`].

mod manifest;
mod report;
mod spec;

pub use manifest::RuntimeManifest;
pub use report::{REPORT_PREFIX, WorkerReport};
pub use spec::WorkerSpec;

/// The contract major this build speaks.
pub const CONTRACT_MAJOR: u32 = 1;
