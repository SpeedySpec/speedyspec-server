//! WordPress's own post-reading tests (tests/php/wordpress/posts, copied from wordpress-develop at 7.2-RC2) pass
//! against the native post functions, beside the native meta functions, on MySQL, run by the pinned PHPUnit inside
//! the embedded PHP. Tests marked `@group documented-break` are excluded, and each must be listed in
//! compat/posts-read/breaks.json's `wordpress_tests`.
//!
//! Two runs, each in its own process (PHP boots once per process): single site, and multisite
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

/// `Class::method` of every test in `tests/php/wordpress/posts` marked `@group documented-break`.
fn marked_breaks() -> Vec<String> {
    let mut found = Vec::new();
    for entry in std::fs::read_dir(root().join("tests/php/wordpress/posts")).unwrap() {
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
/// - `Tests_Post_GetPostStatus`, `test_get_post_parent`, `test_has_post_parent`, `test_get_post_ancestors`,
///   `test_prime_post_parent_id_caches_update`, `test_cache_should_be_invalidated_when_post_name_is_edited`,
///   `test_should_prefer_published_page_then_lowest_id`: they save a post that has a name through
///   `wp_insert_post()` (directly, or through `wp_update_post()`, `wp_trash_post()` or `wp_insert_attachment()`),
///   which runs `get_posts()` (`WP_Query`, not ported) in `wp_add_trashed_suffix_to_post_name_for_trashed_posts()`.
///   `Tests_Post_GetPostStatus` does so in `wpSetUpBeforeClass()`, so its `_post_states()` tests (wp-admin's
///   template.php, not loaded) go too.
/// - `test_get_page_uri_without_argument`, `test_query_caches_post_filter`: `go_to()`, which runs `WP_Query`.
/// - `test_get_posts_caches_post_filter_is_always_raw`: `get_posts()`, which runs `WP_Query`.
/// - `test_get_post_category_property`: `wp_insert_term()` and `wp_set_post_categories()` (taxonomy is not ported).
/// - `test_get_tags_input_property`: `wp_set_post_tags()` makes terms, whose names default-filters.php's
///   `pre_term_name` filters trim (taxonomy is not ported, and its filters are not loaded).
/// - `test_should_return_expected_post_mime_types`: `create_upload_object()` uploads files (media is not loaded).
const EXCLUDE_FILTER: &str = concat!(
    "/Tests_Post_GetPostStatus::",
    "|::(test_get_post_parent|test_has_post_parent|test_get_post_ancestors",
    "|test_prime_post_parent_id_caches_update|test_cache_should_be_invalidated_when_post_name_is_edited",
    "|test_should_prefer_published_page_then_lowest_id",
    "|test_get_page_uri_without_argument|test_query_caches_post_filter",
    "|test_get_posts_caches_post_filter_is_always_raw|test_get_post_category_property",
    "|test_get_tags_input_property|test_should_return_expected_post_mime_types)( |$)/"
);

const RUNS: &[Run] = &[
    Run {
        name: "single_site",
        multisite: false,
        exclude_groups: &["documented-break", "ms-required"],
        min_tests: 192,
    },
    Run {
        name: "multisite",
        multisite: true,
        exclude_groups: &["documented-break", "ms-excluded"],
        min_tests: 192,
    },
];

/// Which run a child process makes.
const RUN_VAR: &str = "SPEEDYSPEC_WORDPRESS_POSTS_RUN";

fn main() {
    let breaks: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(root().join("compat/posts-read/breaks.json")).unwrap(),
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
    let config = root().join("tests/php/phpunit-posts.xml");
    let junit = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("wordpress-posts-{}.junit.xml", run.name));
    std::fs::remove_file(&junit).ok();
    let mut argv = vec![
        "--configuration".to_string(),
        config.display().to_string(),
        "--testsuite".to_string(),
        "wordpress-posts".to_string(),
        "--log-junit".to_string(),
        junit.display().to_string(),
        "--exclude-filter".to_string(),
        EXCLUDE_FILTER.to_string(),
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
