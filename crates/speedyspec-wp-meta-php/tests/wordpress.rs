//! WordPress's own meta tests (tests/php/wordpress/meta, copied from wordpress-develop at 7.2-RC2) pass against the
//! native meta functions on MySQL, run by the pinned PHPUnit inside the embedded PHP. Tests marked
//! `@group documented-break` are excluded, and each must be listed in compat/metadata/breaks.json's
//! `wordpress_tests`.
//!
//! Two runs, each in its own process (PHP boots once per process): single site, and multisite
//! (`WP_TESTS_MULTISITE=1`, `ms-required` included and `ms-excluded` left out).
//!
//! Needs MySQL (`SPEEDYSPEC_TEST_MYSQL_URL`); without it the test reports that it was skipped. The runs use a
//! database of their own (`<database>_metadata_wordpress`), because compat/metadata/setup.php empties its tables.

mod support;

use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};
use speedyspec_wp_meta_php::prepare_release;
use support::check;

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

/// `Class::method` of every test in `tests/php/wordpress/meta` marked `@group documented-break`.
fn marked_breaks() -> Vec<String> {
    let mut found = Vec::new();
    for entry in std::fs::read_dir(root().join("tests/php/wordpress/meta")).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|e| e != "php") {
            continue;
        }
        let text = std::fs::read_to_string(path).unwrap();
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

/// One PHPUnit run: the groups and tests it leaves out, and how many tests it must at least run (an exit status of 0
/// alone does not show that any test ran).
struct Run {
    name: &'static str,
    multisite: bool,
    exclude_groups: &'static [&'static str],
    min_tests: i64,
}

/// Tests left out of both runs because they need code that is not ported or not loaded; they are untested, not
/// different, so none is a documented break. One reason each:
///
/// - `test_user_metadata_not_exists`: `get_users()` and `WP_User_Query` (users are not ported).
/// - `test_meta_type_cast`: `WP_Query` (posts are not ported).
/// - `test_edit_post`: wp-admin's `edit_post()`, which is not loaded.
const EXCLUDE_FILTER: &str =
    "/::(test_user_metadata_not_exists|test_meta_type_cast|test_edit_post)( |$)/";

const RUNS: &[Run] = &[
    Run {
        name: "single_site",
        multisite: false,
        exclude_groups: &["documented-break", "ms-required"],
        min_tests: 339,
    },
    Run {
        name: "multisite",
        multisite: true,
        exclude_groups: &["documented-break", "ms-excluded"],
        min_tests: 343,
    },
];

/// Which run a child process makes.
const RUN_VAR: &str = "SPEEDYSPEC_WORDPRESS_META_RUN";

fn main() {
    let breaks: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(root().join("compat/metadata/breaks.json")).unwrap(),
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

    if let Ok(name) = std::env::var(RUN_VAR) {
        let run = RUNS
            .iter()
            .find(|r| r.name == name)
            .unwrap_or_else(|| panic!("no run named {name}"));
        // The parent made the database and pointed the variable at it.
        let url = std::env::var("SPEEDYSPEC_TEST_MYSQL_URL").expect("the run's database");
        run_one(run, &url);
        return;
    }
    if support::mysql("metadata_wordpress").is_none() {
        return;
    }
    // SPEEDYSPEC_WORDPRESS_META_ONLY=<name> makes one run.
    let only = std::env::var("SPEEDYSPEC_WORDPRESS_META_ONLY").ok();
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
    check("wordpress_meta_runs_pass", failed.is_empty(), &failed);
}

/// Boots the embedded PHP with the native areas and runs one PHPUnit configuration.
fn run_one(run: &Run, url: &str) {
    let release =
        speedyspec_compat::wordpress_release(&speedyspec_compat::area_wordpress("metadata"))
            .expect("the pinned release");
    let prepared = prepare_release(
        &release,
        &release.with_file_name("wordpress-meta-tests-port"),
    )
    .expect("prepared");
    // SAFETY: nothing else runs yet; PHP is not booted and this process has one thread.
    unsafe { std::env::set_var("SPEEDYSPEC_META_WP_ROOT", &prepared) };
    let phar = phpunit();
    let booted = support::boot_with(url, true);
    let config = root().join("tests/php/phpunit-meta.xml");
    let junit = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("wordpress-meta-{}.junit.xml", run.name));
    std::fs::remove_file(&junit).ok();
    let mut argv = vec![
        "--configuration".to_string(),
        config.display().to_string(),
        "--testsuite".to_string(),
        "wordpress-meta".to_string(),
        "--log-junit".to_string(),
        junit.display().to_string(),
        "--exclude-filter".to_string(),
        EXCLUDE_FILTER.to_string(),
    ];
    for group in run.exclude_groups {
        argv.extend(["--exclude-group".to_string(), group.to_string()]);
    }
    let status = booted.php.run_file(&phar, &argv);
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
    check(
        &format!("wordpress_meta_{}_tests_pass", run.name),
        matches!(status, Ok(0)) && tests >= run.min_tests && failures == 0 && errors == 0,
        (&status, tests, failures, errors),
    );
}
