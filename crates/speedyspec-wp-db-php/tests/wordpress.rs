//! WordPress's own database tests (tests/php/wordpress/db*.php, copied from wordpress-develop) pass against the
//! native `wpdb`, run by the pinned PHPUnit inside the embedded PHP on MySQL. Tests marked `@group
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
use speedyspec_data_query::EngineRegistry;
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
    registry
        .configure(EngineConfig {
            name: "default".into(),
            url: url.clone(),
            options: BTreeMap::new(),
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
    let junit = Path::new(env!("CARGO_TARGET_TMPDIR")).join("wordpress-db.junit.xml");
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
    check(
        "wordpress_db_tests_pass",
        matches!(status, Ok(0)) && tests >= FLOOR && failures == 0 && errors == 0,
        (&status, tests, failures, errors),
    );
}
