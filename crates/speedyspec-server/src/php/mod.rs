//! PHP apps: each release gets a pool of `php-worker` processes, each embedding PHP once and answering FastCGI on
//! its own socket; the server's [`PhpRequestHandler`] maps each request to a script and streams it through a worker.

pub mod handler;
pub mod pool;
pub mod runtime;
pub mod runtime_kind;

pub use handler::{InternalError, PhpRequestHandler};
pub use pool::{EmbeddedPhpPool, PoolSettings};
pub use runtime::PhpRuntime;
pub use speedyspec_runtime_contract::WorkerSpec;

/// Every php app's pool settings.
pub fn php_configs(
    config: &crate::config::Config,
) -> std::collections::HashMap<speedyspec_server_domain::AppId, crate::config::PhpConfig> {
    config
        .apps
        .iter()
        .filter_map(|(id, app)| app.php.clone().map(|php| (id.clone(), php)))
        .collect()
}
