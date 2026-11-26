//! WordPress's own option tests (tests/php/wordpress/option, copied from wordpress-develop) pass against the native
//! options API on MySQL, run by the pinned PHPUnit inside the embedded PHP. Tests marked `@group documented-break`
//! are excluded, and each must be listed in compat/options/breaks.json.
//!
//! Four runs, each in its own process (PHP boots once per process): plan 4's option tests; phases 3 and 4's
//! (transients, site and network options, registered settings) single site; the same and `multisite.php` on a
//! multisite install (`WP_TESTS_MULTISITE=1`, `ms-required` included); and transients and options with the object
//! cache's second level on a `file://` engine in a temporary directory.
//!
//! Needs MySQL (`SPEEDYSPEC_TEST_MYSQL_URL`); without it the test reports that it was skipped.

use std::cell::Cell;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

use sha2::{Digest, Sha256};
use speedyspec_cache::{CacheError, CachingStore};
use speedyspec_cache_routing::{OBJECT_CACHE_USE, PersistentCacheSelector};
use speedyspec_container::Container;
use speedyspec_container_php::ContainerArea;
use speedyspec_data::EngineConfig;
use speedyspec_data_file::FileEngineFactory;
use speedyspec_data_mysql::MySqlEngineFactory;
use speedyspec_data_query::EngineRegistry;
use speedyspec_hooks_memory::MemoryHooks;
use speedyspec_php::{BootOptions, NativeArea, Php};
use speedyspec_php_ext::twins_area;
use speedyspec_wp_db::WpDbConfig;
use speedyspec_wp_db_php::WpDbArea;
use speedyspec_wp_hooks_memory::MemoryHookStore;
use speedyspec_wp_hooks_php::WpHooksArea;
use speedyspec_wp_object_cache_php::ObjectCacheArea;
use speedyspec_wp_options_php::{OptionsArea, prepare_release};

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

/// The pinned PHPUnit, fetched once into the cargo target directory and checked by SHA-256 every run.
fn phpunit() -> PathBuf {
    let pin: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(root().join("tests/php/phpunit.json")).unwrap(),
    )
    .unwrap();
    let (url, sha256, version) = (
        pin["url"].as_str().unwrap(),
        pin["sha256"].as_str().unwrap(),
        pin["version"].as_str().unwrap(),
    );
    let path = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("phpunit-{version}.phar"));
    if !path.exists() {
        eprintln!("fetching {url}");
        let bytes = ureq::get(url)
            .call()
            .expect("PHPUnit downloads")
            .into_body()
            .with_config()
            .limit(64 * 1024 * 1024)
            .read_to_vec()
            .expect("PHPUnit downloads");
        std::fs::write(&path, bytes).unwrap();
    }
    let digest = Sha256::digest(std::fs::read(&path).unwrap())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    if digest != sha256 {
        std::fs::remove_file(&path).ok();
        panic!("{}: SHA-256 {digest}, pinned {sha256}", path.display());
    }
    path
}

/// `Class::method` of every test in `tests/php/wordpress/option` marked `@group documented-break`.
fn marked_breaks() -> Vec<String> {
    let mut found = Vec::new();
    for entry in std::fs::read_dir(root().join("tests/php/wordpress/option")).unwrap() {
        let text = std::fs::read_to_string(entry.unwrap().path()).unwrap();
        let class = text
            .lines()
            .find_map(|l| {
                l.strip_prefix("class ")
                    .map(|rest| rest.split_whitespace().next().unwrap().to_string())
            })
            .unwrap_or_default();
        let mut marked = false;
        for line in text.lines() {
            if line.contains("@group documented-break") {
                marked = true;
            } else if marked && line.trim_start().starts_with("public function ") {
                let name = line.trim_start()["public function ".len()..]
                    .split('(')
                    .next()
                    .unwrap()
                    .trim();
                found.push(format!("{class}::{name}"));
                marked = false;
            }
        }
    }
    found
}

/// One PHPUnit run: its test suite (tests/php/phpunit-options.xml), the groups and tests it leaves out, and how many
/// tests it must at least run (an exit status of 0 alone does not show that any test ran).
struct Run {
    name: &'static str,
    suite: &'static str,
    multisite: bool,
    persistent: bool,
    exclude_groups: &'static [&'static str],
    exclude_filter: Option<&'static str>,
    min_tests: i64,
}

const RUNS: &[Run] = &[
    Run {
        name: "options",
        suite: "wordpress-options",
        multisite: false,
        persistent: false,
        exclude_groups: &["documented-break"],
        exclude_filter: None,
        min_tests: 89,
    },
    Run {
        name: "network_options",
        suite: "wordpress-network-options",
        multisite: false,
        persistent: false,
        exclude_groups: &["documented-break", "ms-required"],
        exclude_filter: None,
        min_tests: 71,
    },
    // `test_with_another_site` creates a user and a site (`wp_insert_user()`, `wp_insert_site()`), which are not
    // ported and not loaded; see tests/php/includes/factory.php.
    Run {
        name: "multisite_options",
        suite: "wordpress-multisite-options",
        multisite: true,
        persistent: false,
        exclude_groups: &["documented-break", "ms-excluded"],
        exclude_filter: Some("test_with_another_site"),
        min_tests: 105,
    },
    Run {
        name: "persistent_options",
        suite: "wordpress-persistent-options",
        multisite: false,
        persistent: true,
        exclude_groups: &["documented-break", "ms-required"],
        exclude_filter: None,
        min_tests: 97,
    },
];

/// A Caching Store that counts the reads and writes reaching the one it decorates.
struct Counted {
    inner: Rc<dyn CachingStore>,
    reads: Rc<Cell<u64>>,
    writes: Rc<Cell<u64>>,
}

impl Counted {
    fn read(&self) {
        self.reads.set(self.reads.get() + 1);
    }

    fn write(&self) {
        self.writes.set(self.writes.get() + 1);
    }
}

impl CachingStore for Counted {
    fn name(&self) -> &str {
        self.inner.name()
    }

    fn get(&self, group: &str, key: &str) -> Result<Option<Vec<u8>>, CacheError> {
        self.read();
        self.inner.get(group, key)
    }

    fn get_many(&self, group: &str, keys: &[&str]) -> Result<Vec<Option<Vec<u8>>>, CacheError> {
        self.read();
        self.inner.get_many(group, keys)
    }

    fn set(
        &self,
        group: &str,
        key: &str,
        value: &[u8],
        expiry: Option<Duration>,
    ) -> Result<(), CacheError> {
        self.write();
        self.inner.set(group, key, value, expiry)
    }

    fn add(
        &self,
        group: &str,
        key: &str,
        value: &[u8],
        expiry: Option<Duration>,
    ) -> Result<bool, CacheError> {
        self.write();
        self.inner.add(group, key, value, expiry)
    }

    fn replace(
        &self,
        group: &str,
        key: &str,
        value: &[u8],
        expiry: Option<Duration>,
    ) -> Result<bool, CacheError> {
        self.write();
        self.inner.replace(group, key, value, expiry)
    }

    fn delete(&self, group: &str, key: &str) -> Result<bool, CacheError> {
        self.write();
        self.inner.delete(group, key)
    }

    fn increment(&self, group: &str, key: &str, by: i64) -> Result<Option<i64>, CacheError> {
        self.write();
        self.inner.increment(group, key, by)
    }

    fn decrement(&self, group: &str, key: &str, by: i64) -> Result<Option<i64>, CacheError> {
        self.write();
        self.inner.decrement(group, key, by)
    }

    fn flush(&self) -> Result<(), CacheError> {
        self.inner.flush()
    }

    fn flush_group(&self, group: &str) -> Result<(), CacheError> {
        self.inner.flush_group(group)
    }
}

/// Which run a child process makes.
const RUN_VAR: &str = "SPEEDYSPEC_WORDPRESS_OPTIONS_RUN";

fn main() {
    let breaks: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(root().join("compat/options/breaks.json")).unwrap(),
    )
    .unwrap();
    let listed: Vec<String> = breaks["wordpress_tests"]
        .as_array()
        .map(|tests| {
            tests
                .iter()
                .filter_map(|t| t["test"].as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    let unlisted: Vec<String> = marked_breaks()
        .into_iter()
        .filter(|t| !listed.contains(t))
        .collect();
    check(
        "documented_breaks_are_listed",
        unlisted.is_empty(),
        &unlisted,
    );

    let Ok(url) = std::env::var("SPEEDYSPEC_TEST_MYSQL_URL") else {
        println!("skipped: needs MySQL: set SPEEDYSPEC_TEST_MYSQL_URL");
        return;
    };
    if let Ok(name) = std::env::var(RUN_VAR) {
        let run = RUNS
            .iter()
            .find(|r| r.name == name)
            .unwrap_or_else(|| panic!("no run named {name}"));
        run_one(run, &url);
        return;
    }
    // SPEEDYSPEC_WORDPRESS_OPTIONS_ONLY=<name> makes one run.
    let only = std::env::var("SPEEDYSPEC_WORDPRESS_OPTIONS_ONLY").ok();
    let exe = std::env::current_exe().expect("the test's own path");
    let mut failed = Vec::new();
    for run in RUNS
        .iter()
        .filter(|r| only.as_ref().is_none_or(|o| o == r.name))
    {
        let mut child = std::process::Command::new(&exe);
        child.env(RUN_VAR, run.name);
        if run.multisite {
            child.env("WP_TESTS_MULTISITE", "1");
        } else {
            child.env_remove("WP_TESTS_MULTISITE");
        }
        let status = child.status().expect("the run starts");
        if !status.success() {
            failed.push(run.name);
        }
    }
    check("wordpress_option_runs_pass", failed.is_empty(), &failed);
}

/// Boots the embedded PHP with the native areas and runs one PHPUnit configuration.
fn run_one(run: &Run, url: &str) {
    let release =
        speedyspec_compat::wordpress_release(&speedyspec_compat::area_wordpress("options"))
            .expect("the pinned release");
    let prepared = prepare_release(&release, &release.with_file_name("wordpress-options-port"))
        .expect("prepared");
    // SAFETY: nothing else runs yet; PHP is not booted and this process has one thread.
    unsafe { std::env::set_var("SPEEDYSPEC_OPTIONS_WP_ROOT", &prepared) };
    let phar = phpunit();
    let container = Rc::new(Container::new());
    let hooks = WpHooksArea::new(container.clone(), Rc::new(MemoryHookStore::new())).with_testing();
    let registry = Rc::new(EngineRegistry::new());
    registry.register_factory(Rc::new(MySqlEngineFactory));
    registry
        .configure(EngineConfig {
            name: "default".into(),
            url: url.to_string(),
            options: BTreeMap::new(),
        })
        .unwrap();
    // The persistent run's file engine, routed as `object-cache`; removed when the run ends.
    let (reads, writes) = (Rc::new(Cell::new(0u64)), Rc::new(Cell::new(0u64)));
    let cache_dir = tempfile::tempdir().expect("a temporary directory");
    let persistent = if run.persistent {
        registry.register_factory(Rc::new(FileEngineFactory));
        registry
            .configure(EngineConfig {
                name: "cache".into(),
                url: format!("file://{}", cache_dir.path().display()),
                options: BTreeMap::new(),
            })
            .unwrap();
        registry.route(OBJECT_CACHE_USE, "cache").unwrap();
        let store = PersistentCacheSelector::new()
            .select(&registry, OBJECT_CACHE_USE)
            .expect("the file cache opens");
        check("object_cache_is_persistent", store.is_some(), "not routed");
        store.map(|inner| {
            Rc::new(Counted {
                inner,
                reads: reads.clone(),
                writes: writes.clone(),
            }) as Rc<dyn CachingStore>
        })
    } else {
        None
    };
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
        table_prefix: "wp_".into(),
    };
    let wpdb = WpDbArea::new(container.clone(), hooks.api(), registry, config);
    let cache = ObjectCacheArea::new(container.clone(), persistent);
    let options = OptionsArea::new(container.clone(), hooks.api(), cache.api(), wpdb.api());
    let areas: Vec<Rc<dyn NativeArea>> = vec![
        twins_area(),
        Rc::new(ContainerArea::new("tests", container)),
        Rc::new(hooks),
        Rc::new(wpdb),
        Rc::new(cache),
        Rc::new(options),
    ];
    let php = Php::boot(BootOptions {
        hooks: Rc::new(MemoryHooks::new()),
        preload: &[],
        areas,
    })
    .expect("PHP boots");
    let config = root().join("tests/php/phpunit-options.xml");
    let junit =
        Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("wordpress-{}.junit.xml", run.name));
    std::fs::remove_file(&junit).ok();
    let mut argv = vec![
        "--configuration".to_string(),
        config.display().to_string(),
        "--testsuite".to_string(),
        run.suite.to_string(),
        "--log-junit".to_string(),
        junit.display().to_string(),
    ];
    for group in run.exclude_groups {
        argv.extend(["--exclude-group".to_string(), group.to_string()]);
    }
    if let Some(filter) = run.exclude_filter {
        argv.extend(["--exclude-filter".to_string(), filter.to_string()]);
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
    let (tests, failures, errors, skipped) = (
        total("tests"),
        total("failures"),
        total("errors"),
        total("skipped"),
    );
    println!(
        "{}: {tests} tests, {failures} failures, {errors} errors, {skipped} skipped",
        run.name
    );
    if run.persistent {
        // The second level answered and stored for the tests (the last flush leaves its directory empty).
        check(
            "second_level_was_used",
            reads.get() > 0 && writes.get() > 0,
            (reads.get(), writes.get()),
        );
    }
    check(
        &format!("wordpress_{}_tests_pass", run.name),
        matches!(status, Ok(0)) && tests >= run.min_tests && failures == 0 && errors == 0,
        (&status, tests, failures, errors),
    );
}
