//! Each due cron event runs once across two workers over one MySQL database, with nothing passing between them but
//! the database: the runner's claims are compare-and-swap writes of the `cron` option.

mod support;

use std::path::PathBuf;

use support::{mysql_env, spawn};

fn app() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/cron-run")
}

#[tokio::test(flavor = "current_thread")]
async fn twenty_due_events_run_once_across_two_workers() {
    let Some(env) = mysql_env("cronrun_") else {
        println!("skip: SPEEDYSPEC_TEST_MYSQL_URL is not set");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let app = app();
    let mut a = spawn(dir.path(), &app, None, &env);
    let mut b = spawn(dir.path(), &app, None, &env);
    a.ready().await;
    b.ready().await;
    let seeded = a.get(&app.join("seed.php"), "/seed.php").await;
    assert_eq!(seeded.body(), "21", "{}", seeded.stdout);
    let mut ran = 0;
    for _ in 0..3 {
        let run = app.join("run.php");
        let (from_a, from_b) = tokio::join!(a.get(&run, "/run.php"), b.get(&run, "/run.php"));
        for answer in [from_a, from_b] {
            let summary: serde_json::Value = serde_json::from_str(answer.body())
                .unwrap_or_else(|_| panic!("a run summary: {}", answer.stdout));
            ran += summary["ran"].as_u64().unwrap();
        }
    }
    let check = a.get(&app.join("check.php"), "/check.php").await;
    let state: serde_json::Value =
        serde_json::from_str(check.body()).unwrap_or_else(|_| panic!("{}", check.stdout));
    assert_eq!(state["runs"], 21, "{state}");
    assert_eq!(state["distinct"], 21, "{state}");
    assert_eq!(state["hourly"], serde_json::json!(["future"]), "{state}");
    assert_eq!(ran, 21);
}
