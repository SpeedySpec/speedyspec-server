//! The cron port's endpoint (`speedyspec-cron.php`, which `prepare_release` adds) in a worker over MySQL, on a stub
//! two-site network: the main call runs its own site and lists the other; the follow-up boots as that site and runs
//! it; a call that boots another site answers 409; a visitor gets 404.

mod support;

use std::path::{Path, PathBuf};

use support::{Worker, mysql_env, spawn};

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/cron-endpoint")
}

/// The fixture with the real endpoint beside it, in `dir`.
fn app(dir: &Path) -> PathBuf {
    let app = dir.join("app");
    std::fs::create_dir_all(&app).unwrap();
    for entry in std::fs::read_dir(fixture()).unwrap() {
        let entry = entry.unwrap();
        std::fs::copy(entry.path(), app.join(entry.file_name())).unwrap();
    }
    let endpoint = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../speedyspec-wp-cron-php/src/endpoint.php");
    std::fs::copy(endpoint, app.join("speedyspec-cron.php")).unwrap();
    app
}

async fn cron(worker: &Worker, app: &Path, query: &str, extra: &[(&str, &str)]) -> support::Answer {
    let mut params = vec![("QUERY_STRING", query)];
    params.extend_from_slice(extra);
    worker
        .get_with(
            &app.join("speedyspec-cron.php"),
            &format!("/speedyspec-cron.php?{query}"),
            &params,
        )
        .await
}

#[tokio::test(flavor = "current_thread")]
async fn endpoint_runs_each_site_in_its_own_call() {
    let Some(mut env) = mysql_env("cronep_") else {
        println!("skip: SPEEDYSPEC_TEST_MYSQL_URL is not set");
        return;
    };
    let stubs =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/wordpress-stubs.php");
    env.push((
        "SPEEDYSPEC_TEST_STUBS".into(),
        stubs.to_str().unwrap().into(),
    ));
    let dir = tempfile::tempdir().unwrap();
    let app = app(dir.path());
    let mut worker = spawn(dir.path(), &app, None, &env);
    worker.ready().await;
    let seeded = worker.get(&app.join("seed.php"), "/seed.php").await;
    assert_eq!(seeded.body(), "seeded", "{}", seeded.stdout);

    let visitor = cron(
        &worker,
        &app,
        "budget=30",
        &[("REMOTE_ADDR", "203.0.113.9")],
    )
    .await;
    assert_eq!(visitor.status(), 404, "{}", visitor.stdout);

    let main = cron(&worker, &app, "budget=30", &[]).await;
    let answer: serde_json::Value =
        serde_json::from_str(main.body()).unwrap_or_else(|_| panic!("{}", main.stdout));
    assert_eq!(answer["ran"], 1, "{answer}");
    assert_eq!(
        answer["calls"],
        serde_json::json!(["site=2&host=example.test&path=%2Fsite2%2F"]),
        "{answer}"
    );

    let mapped = cron(
        &worker,
        &app,
        "budget=30&site=2&host=mapped.test&path=%2Fsite2%2F",
        &[],
    )
    .await;
    assert_eq!(mapped.status(), 409, "{}", mapped.stdout);

    let follow_up = format!("budget=30&{}", answer["calls"][0].as_str().unwrap());
    let site = cron(&worker, &app, &follow_up, &[]).await;
    let ran: serde_json::Value =
        serde_json::from_str(site.body()).unwrap_or_else(|_| panic!("{}", site.stdout));
    assert_eq!(ran["ran"], 1, "{ran}");
    assert_eq!(
        ran["calls"],
        serde_json::json!([]),
        "a follow-up lists nothing"
    );

    let check = worker.get(&app.join("check.php"), "/check.php").await;
    assert_eq!(check.body(), r#"["1","2"]"#, "{}", check.stdout);
}
