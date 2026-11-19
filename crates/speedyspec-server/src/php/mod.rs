//! PHP apps: each release gets a pool of `php-worker` processes, each embedding PHP once and answering FastCGI on
//! its own socket; the server's [`PhpRequestHandler`] maps each request to a script and streams it through a worker.

pub mod handler;
pub mod pool;
pub mod runtime;

pub use handler::PhpRequestHandler;
pub use pool::{EmbeddedPhpPool, PoolSettings};
pub use runtime::PhpRuntime;
pub use speedyspec_runtime_contract::WorkerSpec;

/// The PHP runtime binary the pools start: `SPEEDYSPEC_PHP_RUNTIME` when set, else `speedyspec-php-runtime` next to
/// this executable.
pub fn runtime_binary() -> Result<std::path::PathBuf, String> {
    if let Some(path) = std::env::var_os("SPEEDYSPEC_PHP_RUNTIME") {
        return Ok(std::path::PathBuf::from(path));
    }
    let exe = std::env::current_exe().map_err(|err| format!("finding this executable: {err}"))?;
    Ok(exe.with_file_name("speedyspec-php-runtime"))
}
