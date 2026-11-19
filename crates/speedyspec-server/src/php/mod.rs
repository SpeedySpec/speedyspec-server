//! PHP apps: each release gets a pool of `php-worker` processes, each embedding PHP once and answering FastCGI on
//! its own socket; the server's [`PhpRequestHandler`] maps each request to a script and streams it through a worker.

pub mod handler;
pub mod pool;
pub mod runtime;
pub mod worker;

pub use handler::PhpRequestHandler;
pub use pool::{EmbeddedPhpPool, PoolSettings};
pub use runtime::PhpRuntime;
pub use worker::{WorkerSpec, run_worker};

/// The prefix of the worker's after-request report on `FCGI_STDERR`.
pub const REPORT_PREFIX: &[u8] = b"\0speedyspec:";
