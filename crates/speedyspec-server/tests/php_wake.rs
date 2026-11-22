//! What a worker reports after a request reaches the app's wake sink: when the app next wants its cron call.

mod support;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use bytes::Bytes;
use http::{HeaderMap, Method};
use speedyspec_runtime_contract::{CONTRACT_MAJOR, WorkerSpec};
use speedyspec_server::php::PhpRequestHandler;
use speedyspec_server::php::pool::{
    EmbeddedPhpPool, InternalLane, PoolSettings, prepare_socket_dir,
};

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
async fn next_cron_reaches_the_sink() {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let sink = seen.clone();
    let handler = handler(1).with_wake_sink(Arc::new(move |t| sink.lock().unwrap().push(t)));
    for lane in [InternalLane::Cron, InternalLane::Calls] {
        let response = handler
            .internal(
                "/",
                "localhost",
                Method::GET,
                "/wake.php",
                HeaderMap::new(),
                Bytes::new(),
                Duration::from_secs(5),
                lane,
            )
            .await
            .unwrap();
        assert_eq!(response.body().as_ref(), b"woke");
    }
    let plain = handler
        .internal(
            "/",
            "localhost",
            Method::GET,
            "/internal.php",
            HeaderMap::new(),
            Bytes::new(),
            Duration::from_secs(5),
            InternalLane::Calls,
        )
        .await
        .unwrap();
    assert_eq!(plain.status(), 200);
    // Two reports carried a wake time; the third request reported none.
    assert_eq!(*seen.lock().unwrap(), vec![4242, 4242]);
}
