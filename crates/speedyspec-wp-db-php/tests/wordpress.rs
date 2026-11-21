//! WordPress's own database tests (tests/php/wordpress/db*.php, copied from wordpress-develop) pass against the
//! native `wpdb`, run by the pinned PHPUnit inside the embedded PHP on MySQL. With `SPEEDYSPEC_DB_URL` set to a
//! `sqlite://` or `postgres://` URL they run on that engine instead, where only the tests
//! `compat/phpunit-<engine>.json` lists may fail. Tests marked `@group
//! documented-break` or `@group needs-wordpress` are excluded, and each must be listed in compat/wp-db/breaks.json
//! under `wordpress_tests`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use sha2::{Digest, Sha256};
use speedyspec_container::Container;
use speedyspec_container_php::ContainerArea;
use speedyspec_data::EngineConfig;
use speedyspec_data_mysql::MySqlEngineFactory;
use speedyspec_data_postgres::PostgresEngineFactory;
use speedyspec_data_query::EngineRegistry;
use speedyspec_data_sqlite::SqliteEngineFactory;
use speedyspec_hooks_memory::MemoryHooks;
use speedyspec_php::{BootOptions, NativeArea, Php};
use speedyspec_php_ext::twins_area;
use speedyspec_wp_db::WpDbConfig;
use speedyspec_wp_db_php::WpDbArea;
use speedyspec_wp_hooks_memory::MemoryHookStore;
use speedyspec_wp_hooks_php::WpHooksArea;

/// The first green run's count (WordPress's 118 test methods with their data sets, less the excluded one); never lower.
const FLOOR: i64 = 652;

fn check(name: &str, ok: bool, detail: impl std::fmt::Debug) {
    if ok {
        println!("ok: {name}");
    } else {
        eprintln!("FAIL: {name}: {detail:?}");
        std::process::exit(1);
    }
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The pinned PHPUnit, in the cargo target directory, checked by SHA-256 (the hook tests fetch it).
fn phpunit() -> PathBuf {
    let pin: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(root().join("tests/php/phpunit.json")).unwrap(),
    )
    .unwrap();
    let (sha256, version) = (
        pin["sha256"].as_str().unwrap(),
        pin["version"].as_str().unwrap(),
    );
    let path = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("phpunit-{version}.phar"));
    let bytes = std::fs::read(&path).unwrap_or_else(|_| {
        panic!(
            "{} is missing: run the hook tests once to fetch it",
            path.display()
        )
    });
    let digest = format!("{:x}", Sha256::digest(bytes));
    assert_eq!(
        digest,
        sha256,
        "{}: SHA-256 differs from the pin",
        path.display()
    );
    path
}

fn files() -> Vec<PathBuf> {
    let base = root().join("tests/php/wordpress");
    vec![
        base.join("db.php"),
        base.join("db/charset.php"),
        base.join("db/realEscape.php"),
        base.join("db/dbDelta.php"),
    ]
}

/// `Class::method` of every test marked with one of the excluding groups.
fn marked() -> Vec<String> {
    let mut found = Vec::new();
    for file in files() {
        let text = std::fs::read_to_string(&file).unwrap();
        let class = text
            .lines()
            .find_map(|l| {
                l.strip_prefix("class ")
                    .map(|rest| rest.split_whitespace().next().unwrap().to_string())
            })
            .unwrap_or_default();
        let mut flagged = false;
        for line in text.lines() {
            if line.contains("@group documented-break") || line.contains("@group needs-wordpress") {
                flagged = true;
            } else if flagged && line.trim_start().starts_with("public function ") {
                let name = line.trim_start()["public function ".len()..]
                    .split('(')
                    .next()
                    .unwrap()
                    .trim();
                found.push(format!("{class}::{name}"));
                flagged = false;
            }
        }
    }
    found
}

/// `Class::method` (with its data set) of every test case the JUnit log shows failing or erroring.
fn failed_tests(log: &str) -> Vec<String> {
    let mut out = Vec::new();
    for case in log.split("<testcase ").skip(1) {
        let head = case.split('>').next().unwrap_or_default();
        if head.ends_with('/') {
            continue;
        }
        let body = case.split("</testcase>").next().unwrap_or_default();
        if !(body.contains("<failure") || body.contains("<error")) {
            continue;
        }
        let attr = |name: &str| {
            let marker = format!("{name}=\"");
            head.find(&marker)
                .map(|i| &head[i + marker.len()..])
                .and_then(|rest| rest.split('"').next())
                .unwrap_or_default()
                .to_string()
        };
        out.push(format!("{}::{}", attr("class"), attr("name")).replace("&quot;", "\""));
    }
    out
}

/// The tests `compat/phpunit-<engine>.json` lists as failing on that engine.
fn expected_failures(engine: &str) -> Vec<String> {
    let path = root().join(format!("compat/phpunit-{engine}.json"));
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let value: serde_json::Value = serde_json::from_str(&text).unwrap();
    value["expected_failures"]
        .as_array()
        .map(|list| {
            list.iter()
                .filter_map(|e| e["test"].as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

fn main() {
    let breaks: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(root().join("compat/wp-db/breaks.json")).unwrap(),
    )
    .unwrap();
    let listed: Vec<String> = breaks["wordpress_tests"]
        .as_array()
        .map(|t| {
            t.iter()
                .filter_map(|t| t["test"].as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    let marked = marked();
    let unlisted: Vec<&String> = marked.iter().filter(|t| !listed.contains(t)).collect();
    check(
        "documented_breaks_are_listed",
        unlisted.is_empty(),
        &unlisted,
    );

    let Ok(url) = std::env::var("SPEEDYSPEC_TEST_MYSQL_URL") else {
        println!("skipped: needs MySQL: set SPEEDYSPEC_TEST_MYSQL_URL");
        return;
    };
    let phar = phpunit();
    let container = Rc::new(Container::new());
    let hooks = WpHooksArea::new(container.clone(), Rc::new(MemoryHookStore::new())).with_testing();
    let registry = Rc::new(EngineRegistry::new());
    registry.register_factory(Rc::new(MySqlEngineFactory));
    registry.register_factory(Rc::new(SqliteEngineFactory));
    registry.register_factory(Rc::new(PostgresEngineFactory));
    let engine_url = std::env::var("SPEEDYSPEC_DB_URL").unwrap_or_else(|_| url.clone());
    let engine = engine_url
        .split("://")
        .next()
        .unwrap_or("mysql")
        .to_string();
    // Another engine than MySQL: only the tests compat/phpunit-<engine>.json lists may fail.
    let sqlite = engine != "mysql";
    let mut options = BTreeMap::new();
    if engine == "sqlite" {
        options.insert(
            "database".to_string(),
            url.rsplit('/').next().unwrap_or("wordpress").to_string(),
        );
    }
    if engine == "postgres" {
        let mut admin =
            postgres::Client::connect(&engine_url, postgres::NoTls).expect("PostgreSQL is up");
        admin
            .batch_execute("DROP SCHEMA IF EXISTS t_wp_phpunit CASCADE")
            .unwrap();
        options.insert("schema".to_string(), "t_wp_phpunit".to_string());
    }
    registry
        .configure(EngineConfig {
            name: "default".into(),
            url: engine_url,
            options,
        })
        .unwrap();
    let rest = url.trim_start_matches("mysql://");
    let (auth, host_db) = rest.split_once('@').expect("user@host");
    let (user, password) = auth.split_once(':').unwrap_or((auth, ""));
    let (host, name) = host_db.split_once('/').expect("host/database");
    let config = WpDbConfig {
        user: user.into(),
        password: password.into(),
        name: name.into(),
        host: host.into(),
        charset: Some("utf8mb4".into()),
        collate: Some(String::new()),
        table_prefix: "wptests_".into(),
    };
    let wpdb = WpDbArea::new(container.clone(), hooks.api(), registry, config);
    let areas: Vec<Rc<dyn NativeArea>> = vec![
        twins_area(),
        Rc::new(ContainerArea::new("tests", container)),
        Rc::new(hooks),
        Rc::new(wpdb),
    ];
    let php = Php::boot(BootOptions {
        hooks: Rc::new(MemoryHooks::new()),
        preload: &[],
        areas,
    })
    .expect("PHP boots");
    let junit = Path::new(env!("CARGO_TARGET_TMPDIR")).join(if sqlite {
        format!("wordpress-db.{engine}.junit.xml")
    } else {
        "wordpress-db.junit.xml".to_string()
    });
    std::fs::remove_file(&junit).ok();
    let argv = vec![
        "--configuration".to_string(),
        root()
            .join("tests/php/wp-db/phpunit.xml")
            .display()
            .to_string(),
        "--log-junit".to_string(),
        junit.display().to_string(),
    ];
    let mut argv = argv;
    // A filter for looking at one test while working on the port.
    if let Ok(filter) = std::env::var("SPEEDYSPEC_PHPUNIT_FILTER") {
        argv.extend(["--filter".to_string(), filter]);
    }
    let status = php.run_file(&phar, &argv);
    let log = std::fs::read_to_string(&junit).unwrap_or_default();
    let total = |name: &str| -> i64 {
        let marker = format!("{name}=\"");
        log.find("<testsuite ")
            .and_then(|at| log[at..].find(&marker).map(|i| at + i + marker.len()))
            .and_then(|start| log[start..].split('"').next())
            .and_then(|n| n.parse().ok())
            .unwrap_or(-1)
    };
    let (tests, failures, errors) = (total("tests"), total("failures"), total("errors"));
    if sqlite {
        let failed = failed_tests(&log);
        let expected = expected_failures(&engine);
        let unexpected: Vec<&String> = failed.iter().filter(|t| !expected.contains(t)).collect();
        let stale: Vec<&String> = expected.iter().filter(|t| !failed.contains(t)).collect();
        for t in &unexpected {
            eprintln!("FAILED ON {}: {t}", engine.to_ascii_uppercase());
        }
        check(
            &format!("wordpress_db_tests_pass_on_{engine}"),
            tests >= FLOOR && unexpected.is_empty(),
            (tests, &unexpected),
        );
        check(
            &format!("phpunit_{engine}_entries_still_fail"),
            stale.is_empty(),
            &stale,
        );
        return;
    }
    check(
        "wordpress_db_tests_pass",
        matches!(status, Ok(0)) && tests >= FLOOR && failures == 0 && errors == 0,
        (&status, tests, failures, errors),
    );
}
