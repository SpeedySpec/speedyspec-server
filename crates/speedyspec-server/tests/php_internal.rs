//! The server's own requests to a PHP app (health probes, the gate's questions), on the pool's internal worker.

mod support;

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use bytes::Bytes;
use http::{HeaderMap, Method};
use speedyspec_runtime_contract::{CONTRACT_MAJOR, WorkerSpec};
use speedyspec_server::php::pool::{EmbeddedPhpPool, PoolSettings, prepare_socket_dir};
use speedyspec_server::php::{InternalError, PhpRequestHandler};

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/php-app")
}

fn handler(max: u32) -> PhpRequestHandler {
    let binary = support::runtime::binary();
    let release = std::fs::canonicalize(fixture()).unwrap();
    let spec = WorkerSpec {
        contract: CONTRACT_MAJOR,
        app: "blog".into(),
        socket: PathBuf::new(),
        release_dir: release.clone(),
        site_dir: None,
        ini: Default::default(),
        bindings: Vec::new(),
        env: Vec::new(),
    };
    let settings = PoolSettings {
        min: 1,
        max,
        max_requests: 100,
        request_timeout: Duration::from_secs(10),
        memory_limit: u64::MAX,
        idle_timeout: Duration::from_secs(60),
    };
    let sockets = prepare_socket_dir(Path::new("/nonexistent"), "blog", "1.0.0").unwrap();
    let pool = EmbeddedPhpPool::start(binary, spec, sockets, settings, "1.0.0".into()).unwrap();
    PhpRequestHandler::new(pool, &release.join("public"), "index.php", None).unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn internal_request_runs_while_visitors_fill_the_pool() {
    let handler = handler(1);
    let visitor = handler.pool().acquire().await.unwrap();
    let started = Instant::now();
    let response = handler
        .internal(
            "/",
            "localhost",
            Method::POST,
            "/internal.php",
            HeaderMap::new(),
            Bytes::from_static(b"hi"),
            Duration::from_secs(5),
            speedyspec_server::php::pool::InternalLane::Calls,
        )
        .await
        .unwrap();
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "no wait for the visitor ({:?})",
        started.elapsed()
    );
    assert_eq!(response.status(), 200);
    assert_eq!(response.body().as_ref(), b"internal ok POST localhost hi");
    drop(visitor);
}

#[tokio::test(flavor = "multi_thread")]
async fn internal_request_times_out() {
    let handler = handler(1);
    let err = handler
        .internal(
            "/",
            "localhost",
            Method::GET,
            "/sleep.php?s=3",
            HeaderMap::new(),
            Bytes::new(),
            Duration::from_millis(300),
            speedyspec_server::php::pool::InternalLane::Calls,
        )
        .await
        .unwrap_err();
    assert!(matches!(err, InternalError::Timeout), "{err:?}");
}
