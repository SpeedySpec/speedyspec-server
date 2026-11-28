//! WordPress's own post tests (copied from wordpress-develop at 7.2-RC2) pass against the native post functions,
//! beside the native meta functions, on MySQL, run by the pinned PHPUnit inside the embedded PHP:
//!
//! - the post-reading tests (tests/php/wordpress/posts), whose factory stores posts as rows;
//! - the post-writing tests (tests/php/wordpress/posts/write), whose factory makes posts through the native
//!   `wp_insert_post()`.
//!
//! Tests marked `@group documented-break` are excluded, and each must be listed in its area's breaks.json
//! (compat/posts-read or compat/posts-write) under `wordpress_tests`.
//!
//! Each suite runs twice, each run in its own process (PHP boots once per process): single site, and multisite
//! (`WP_TESTS_MULTISITE=1`, `ms-required` included and `ms-excluded` left out).
//!
//! Needs MySQL (`SPEEDYSPEC_TEST_MYSQL_URL`); without it the test reports that it was skipped. The runs use a
//! database of their own (`<database>_posts_wordpress`), because compat/posts-read/setup.php empties its tables.

mod support;

use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};
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

/// `Class::method` of every test directly in `dir` (under tests/php) marked `@group documented-break`.
fn marked_breaks(dir: &str) -> Vec<String> {
    let mut found = Vec::new();
    for entry in std::fs::read_dir(root().join("tests/php").join(dir)).unwrap() {
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

/// One suite of WordPress's tests: where they are, the PHPUnit configuration and test suite that runs them, the area
/// whose breaks.json lists its documented breaks, and the tests it leaves out.
struct Suite {
    dir: &'static str,
    config: &'static str,
    testsuite: &'static str,
    area: &'static str,
    exclude_filter: &'static str,
}

/// One PHPUnit run: its suite, the groups it leaves out, and how many tests it must at least run (an exit status of 0
/// alone does not show that any test ran).
struct Run {
    name: &'static str,
    suite: &'static Suite,
    multisite: bool,
    exclude_groups: &'static [&'static str],
    min_tests: i64,
}

/// Tests left out of both runs because they need code that is not ported or not loaded; they are untested, not
/// different, so none is a documented break. One reason each:
///
/// - `test_get_page_uri_without_argument`, `test_query_caches_post_filter`: `go_to()`, which needs WordPress's whole
///   request (`WP::main()`, the template loader), not loaded.
/// - `test_get_post_category_property`: `wp_insert_term()` and `wp_set_post_categories()` (taxonomy is not ported).
/// - `test_get_tags_input_property`: `wp_set_post_tags()` makes terms, whose names default-filters.php's
///   `pre_term_name` filters trim (taxonomy is not ported, and its filters are not loaded).
/// - `test_should_return_expected_post_mime_types`: `create_upload_object()` uploads files (media is not loaded).
const READ_EXCLUDE_FILTER: &str = concat!(
    "/::(test_get_page_uri_without_argument|test_query_caches_post_filter",
    "|test_get_post_category_property|test_get_tags_input_property",
    "|test_should_return_expected_post_mime_types)( |$)/"
);

const READ: Suite = Suite {
    dir: "wordpress/posts",
    config: "tests/php/phpunit-posts.xml",
    testsuite: "wordpress-posts",
    area: "posts-read",
    exclude_filter: READ_EXCLUDE_FILTER,
};

/// Tests of the post-writing suite left out of both runs because they need code that is not ported or not loaded;
/// they are untested, not different, so none is a documented break. One reason each:
///
/// - `test_wp_tag_cloud_link_with_post_type`: `wp_tag_cloud()` translates its counts with l10n.php, not loaded.
/// - `test_post_title_with_percentage_followed_by_numbers`: `go_to()`, which needs WordPress's whole request.
/// - `test_wp_insert_post_for_customize_changeset_should_not_drop_post_name`: the customizer's
///   `_wp_customize_changeset_filter_insert_post_data()` and `WP_Customize_Manager`, not loaded.
/// - `test_update_via_rest_controller`, `test_new_post_via_rest_controller`,
///   `test_update_attachment_via_rest_controller`: the REST server and requests, not loaded.
const WRITE_EXCLUDE_FILTER: &str = concat!(
    "/::(test_wp_tag_cloud_link_with_post_type|test_post_title_with_percentage_followed_by_numbers",
    "|test_wp_insert_post_for_customize_changeset_should_not_drop_post_name",
    "|test_update_via_rest_controller|test_new_post_via_rest_controller",
    "|test_update_attachment_via_rest_controller)( |$)/"
);

const WRITE: Suite = Suite {
    dir: "wordpress/posts/write",
    config: "tests/php/phpunit-posts-write.xml",
    testsuite: "wordpress-posts-write",
    area: "posts-write",
    exclude_filter: WRITE_EXCLUDE_FILTER,
};

const RUNS: &[Run] = &[
    Run {
        name: "single_site",
        suite: &READ,
        multisite: false,
        exclude_groups: &["documented-break", "ms-required"],
        min_tests: 235,
    },
    Run {
        name: "multisite",
        suite: &READ,
        multisite: true,
        exclude_groups: &["documented-break", "ms-excluded"],
        min_tests: 235,
    },
    Run {
        name: "write_single_site",
        suite: &WRITE,
        multisite: false,
        exclude_groups: &["documented-break", "ms-required"],
        min_tests: 213,
    },
    Run {
        name: "write_multisite",
        suite: &WRITE,
        multisite: true,
        exclude_groups: &["documented-break", "ms-excluded"],
        min_tests: 213,
    },
];

/// Which run a child process makes.
const RUN_VAR: &str = "SPEEDYSPEC_WORDPRESS_POSTS_RUN";

/// The documented breaks of `suite` that its area's breaks.json does not list.
fn unlisted_breaks(suite: &Suite) -> Vec<String> {
    let breaks: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(root().join(format!("compat/{}/breaks.json", suite.area)))
            .unwrap(),
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
    marked_breaks(suite.dir)
        .into_iter()
        .filter(|t| !listed.contains(t))
        .collect()
}

fn main() {
    let unlisted: Vec<String> = [&READ, &WRITE]
        .into_iter()
        .flat_map(unlisted_breaks)
        .collect();
    if !check(
        "documented_breaks_are_listed",
        unlisted.is_empty(),
        &unlisted,
    ) {
        std::process::exit(1);
    }

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
    if support::mysql("posts_wordpress").is_none() {
        return;
    }
    // SPEEDYSPEC_WORDPRESS_POSTS_ONLY=<name> makes one run.
    let only = std::env::var("SPEEDYSPEC_WORDPRESS_POSTS_ONLY").ok();
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
    if !check("wordpress_posts_runs_pass", failed.is_empty(), &failed) {
        std::process::exit(1);
    }
}

/// Boots the embedded PHP with the native areas and runs one PHPUnit configuration.
fn run_one(run: &Run, url: &str) {
    let (_release, prepared) = support::prepared("posts-tests-port");
    // SAFETY: nothing else runs yet; PHP is not booted and this process has one thread.
    unsafe { std::env::set_var("SPEEDYSPEC_POSTS_WP_ROOT", &prepared) };
    let phar = phpunit();
    let (php, _container) = support::boot_with(url, true);
    let config = root().join(run.suite.config);
    let junit = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("wordpress-posts-{}.junit.xml", run.name));
    std::fs::remove_file(&junit).ok();
    let mut argv = vec![
        "--configuration".to_string(),
        config.display().to_string(),
        "--testsuite".to_string(),
        run.suite.testsuite.to_string(),
        "--log-junit".to_string(),
        junit.display().to_string(),
        "--exclude-filter".to_string(),
        run.suite.exclude_filter.to_string(),
    ];
    for group in run.exclude_groups {
        argv.extend(["--exclude-group".to_string(), group.to_string()]);
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
    if !check(
        &format!("wordpress_posts_{}_tests_pass", run.name),
        matches!(status, Ok(0)) && tests >= run.min_tests && failures == 0 && errors == 0,
        (&status, tests, failures, errors),
    ) {
        std::process::exit(1);
    }
}
