//! WordPress's own password tests (tests/php/wordpress/password, copied from wordpress-develop) run against the native
//! functions, with WordPress's other functions from the pinned release, by the pinned PHPUnit inside the embedded PHP.
//! The tests in [`EXPECTED_FAILURES`] fail on purpose.

use std::path::{Path, PathBuf};
use std::rc::Rc;

use sha2::{Digest, Sha256};
use speedyspec_compat::wordpress_release;
use speedyspec_container::Container;
use speedyspec_container_php::ContainerArea;
use speedyspec_hooks_memory::MemoryHooks;
use speedyspec_php::{BootOptions, NativeArea, Php};
use speedyspec_wp_crypto_sodium::Sodium;
use speedyspec_wp_hooks_memory::MemoryHookStore;
use speedyspec_wp_hooks_php::WpHooksArea;
use speedyspec_wp_password_php::PasswordArea;

/// WordPress tests SpeedySpec fails on purpose, in the order they run:
///
/// - phpass and raw MD5 hashes never verify (Jacob, 2026-11-21 and 2026-11-22);
/// - argon2id is the default, so the three tests that tune bcrypt's cost through `wp_hash_password_options` and expect
///   a bcrypt hash (or a rehash after a cost change) see an argon2id hash, which ignores `cost` (Jacob, 2026-11-22).
const EXPECTED_FAILURES: &[&str] = &[
    "test_wp_check_password_supports_phpass_hash",
    "test_wp_check_password_supports_hash_with_increased_bcrypt_cost",
    "test_wp_check_password_supports_hash_with_reduced_bcrypt_cost",
    "test_wp_check_password_supports_md5_hash",
    "test_password_hashing_options_can_be_filtered",
];

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

fn main() {
    let release = wordpress_release(&speedyspec_compat::pinned_wordpress())
        .expect("the pinned WordPress release");
    // SAFETY: set before PHP boots, while this process has one thread.
    unsafe { std::env::set_var("SPEEDYSPEC_WP_ROOT", &release) };
    let phar = phpunit();
    let container = Rc::new(Container::new());
    let hooks = WpHooksArea::new(container.clone(), Rc::new(MemoryHookStore::new())).with_testing();
    let sodium = Rc::new(Sodium::init().expect("libsodium initializes"));
    let password = PasswordArea::new(hooks.api(), container.clone(), sodium);
    password.bind_defaults();
    let container_area = PasswordArea::ports(ContainerArea::new("test", container));
    let areas: Vec<Rc<dyn NativeArea>> = vec![
        speedyspec_php_ext::twins_area(),
        Rc::new(container_area),
        Rc::new(hooks),
        Rc::new(password),
    ];
    let php = Php::boot(BootOptions {
        hooks: Rc::new(MemoryHooks::new()),
        preload: &[],
        areas,
    })
    .expect("PHP boots");
    let config = root().join("tests/php/phpunit-password.xml");
    let junit = Path::new(env!("CARGO_TARGET_TMPDIR")).join("wordpress-password.junit.xml");
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
    let mut failed: Vec<&str> = log
        .split("<testcase ")
        .skip(1)
        .filter(|case| case.contains("<failure") || case.contains("<error"))
        .filter_map(|case| case.split("name=\"").nth(1)?.split('"').next())
        .map(|name| name.split(" with data set").next().unwrap_or(name))
        .collect();
    failed.dedup();
    check(
        "wordpress_password_tests_pass_but_divergences",
        tests >= 30
            && errors == 0
            && failures == EXPECTED_FAILURES.len() as i64
            && failed == EXPECTED_FAILURES,
        (&status, tests, failures, errors, &failed),
    );
}
