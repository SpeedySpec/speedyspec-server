//! Workers with database credentials serve WordPress's option functions natively, over MySQL.

mod support;

use std::path::PathBuf;
use std::time::{Duration, Instant};

use support::{mysql_env, release, spawn};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

#[tokio::test(flavor = "current_thread")]
async fn worker_serves_native_options() {
    let Some(env) = mysql_env("cronworker_") else {
        println!("skip: SPEEDYSPEC_TEST_MYSQL_URL is not set");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let app = fixture("options-app");
    let mut worker = spawn(dir.path(), &app, None, &env);
    worker.ready().await;
    let answer = worker.get(&app.join("index.php"), "/").await;
    assert_eq!(
        answer.body(),
        "speedyspec://wp-options|from-worker",
        "{}",
        answer.stdout
    );
}

#[tokio::test(flavor = "current_thread")]
async fn unreachable_database_refuses_to_serve() {
    let dir = tempfile::tempdir().unwrap();
    let app = release(dir.path(), "app", "<?php echo 'served';");
    let env = vec![
        ("DB_NAME".to_string(), "x".to_string()),
        ("DB_HOST".to_string(), "127.0.0.1:1".to_string()),
    ];
    let mut worker = spawn(dir.path(), &app, None, &env);
    let started = Instant::now();
    let status = loop {
        if let Some(status) = worker.child.try_wait().unwrap() {
            break status;
        }
        assert!(
            started.elapsed() < Duration::from_secs(30),
            "the worker kept running"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    };
    let stderr = worker.stderr_text();
    assert_eq!(status.code(), Some(1), "{stderr}");
    assert!(
        stderr.contains("php_worker_database_unreachable"),
        "{stderr}"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn drop_in_is_refused() {
    let Some(env) = mysql_env("cronworker_") else {
        println!("skip: SPEEDYSPEC_TEST_MYSQL_URL is not set");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let app = release(dir.path(), "app", "<?php echo 'served';");
    let site = dir.path().join("site");
    std::fs::create_dir_all(site.join("wp-content")).unwrap();
    std::fs::write(site.join("wp-content/db.php"), "<?php").unwrap();
    let mut worker = spawn(dir.path(), &app, Some(&site), &env);
    worker.ready().await;
    let answer = worker.get(&app.join("index.php"), "/").await;
    assert_eq!(answer.status(), 500, "{}", answer.stdout);
    assert!(answer.body().contains("db.php"), "{}", answer.stdout);
    assert!(!answer.body().contains("served"), "{}", answer.stdout);
}

#[tokio::test(flavor = "current_thread")]
async fn no_db_name_boots_as_before() {
    let dir = tempfile::tempdir().unwrap();
    let app = release(
        dir.path(),
        "app",
        "<?php echo 'ok', function_exists('get_option') ? '+options' : '';",
    );
    let mut worker = spawn(dir.path(), &app, None, &[]);
    worker.ready().await;
    let answer = worker.get(&app.join("index.php"), "/").await;
    assert_eq!(answer.body(), "ok", "{}", answer.stdout);
}

#[tokio::test(flavor = "current_thread")]
async fn release_drop_in_is_refused_without_a_site_dir() {
    let Some(env) = mysql_env("cronworker_") else {
        println!("skip: SPEEDYSPEC_TEST_MYSQL_URL is not set");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let app = release(dir.path(), "app", "<?php echo 'served';");
    // Without a site directory, WP_CONTENT_DIR is the release's own wp-content.
    std::fs::create_dir_all(app.join("wp-content")).unwrap();
    std::fs::write(app.join("wp-content/db.php"), "<?php").unwrap();
    let mut worker = spawn(dir.path(), &app, None, &env);
    worker.ready().await;
    for _ in 0..2 {
        let answer = worker.get(&app.join("index.php"), "/").await;
        assert_eq!(answer.status(), 500, "{}", answer.stdout);
        assert!(answer.body().contains("db.php"), "{}", answer.stdout);
    }
}

#[tokio::test(flavor = "current_thread")]
async fn worker_serves_native_cron() {
    let Some(env) = mysql_env("cronworker_") else {
        println!("skip: SPEEDYSPEC_TEST_MYSQL_URL is not set");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let app = fixture("cron-app");
    let mut worker = spawn(dir.path(), &app, None, &env);
    worker.ready().await;
    let answer = worker.get(&app.join("index.php"), "/").await;
    assert_eq!(
        answer.body(),
        "speedyspec://wp-cron|scheduled",
        "{}",
        answer.stdout
    );
}
