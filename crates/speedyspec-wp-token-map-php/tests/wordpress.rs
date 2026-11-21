//! WordPress 7.1.5's own token map test (`tests/php/wordpress/token-map`, copied from wordpress-develop at tag 7.1.5)
//! passes against the token map, run by the pinned PHPUnit inside the embedded PHP. Tests marked
//! `@group documented-break` are excluded, and each must be listed in compat/token-map/breaks.json.

use std::path::{Path, PathBuf};
use std::rc::Rc;

use sha2::{Digest, Sha256};
use speedyspec_container::Container;
use speedyspec_hooks_memory::MemoryHooks;
use speedyspec_php::{BootOptions, NativeArea, Php};
use speedyspec_php_ext::twins_area;
use speedyspec_wp_hooks_memory::MemoryHookStore;
use speedyspec_wp_hooks_php::WpHooksArea;
use speedyspec_wp_token_map_php::{TokenMapArea, prepare_release};

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
    let digest = format!("{:x}", Sha256::digest(std::fs::read(&path).unwrap()));
    if digest != sha256 {
        std::fs::remove_file(&path).ok();
        panic!("{}: SHA-256 {digest}, pinned {sha256}", path.display());
    }
    path
}

/// `Class::method` of every test marked `@group documented-break` in the copied tests.
fn marked_breaks() -> Vec<String> {
    let mut found = Vec::new();
    for dir in ["tests/php/wordpress/token-map"] {
        for entry in std::fs::read_dir(root().join(dir)).unwrap() {
            let text = std::fs::read_to_string(entry.unwrap().path()).unwrap();
            let class = text
                .lines()
                .find_map(|l| {
                    l.strip_prefix("class ")
                        .map(|r| r.split_whitespace().next().unwrap().to_string())
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
    }
    found
}

fn main() {
    let breaks: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(root().join("compat/token-map/breaks.json")).unwrap(),
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
    let unlisted: Vec<String> = marked_breaks()
        .into_iter()
        .filter(|t| !listed.contains(t))
        .collect();
    check(
        "documented_breaks_are_listed",
        unlisted.is_empty(),
        &unlisted,
    );

    let release = speedyspec_compat::wordpress_release("7.1.5").expect("WordPress 7.1.5");
    let prepared = prepare_release(
        &release,
        &release.with_file_name("wordpress-token-map-port"),
    )
    .expect("release prepared");
    // SAFETY: set before PHP boots, while this process has one thread.
    unsafe { std::env::set_var("SPEEDYSPEC_TOKEN_MAP_WP_ROOT", &prepared) };

    let phar = phpunit();
    let container = Rc::new(Container::new());
    let hooks = WpHooksArea::new(container, Rc::new(MemoryHookStore::new())).with_testing();
    let areas: Vec<Rc<dyn NativeArea>> = vec![twins_area(), Rc::new(hooks), Rc::new(TokenMapArea)];
    let php = Php::boot(BootOptions {
        hooks: Rc::new(MemoryHooks::new()),
        preload: &[],
        areas,
    })
    .expect("PHP boots");
    let config = root().join("tests/php/phpunit-token-map.xml");
    let junit = Path::new(env!("CARGO_TARGET_TMPDIR")).join("wordpress-token-map.junit.xml");
    std::fs::remove_file(&junit).ok();
    let argv = vec![
        "--configuration".to_string(),
        config.display().to_string(),
        "--log-junit".to_string(),
        junit.display().to_string(),
    ];
    let status = php.run_file(&phar, &argv);
    // The suite's own totals: an exit status of 0 alone does not show that any test ran.
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
        "wordpress_token_map_tests_pass",
        matches!(status, Ok(0)) && tests >= 2000 && failures == 0 && errors == 0,
        (&status, tests, failures, errors),
    );
}
