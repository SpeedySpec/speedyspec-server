//! WordPress's own Abilities API tests (tests/php/wordpress/abilities, copied from wordpress-develop's 7.1 branch) run
//! against the WP_Ability shim and the Rust Abilities server, with WordPress's other functions from the pinned release,
//! by the pinned PHPUnit inside the embedded PHP. The tests in [`EXPECTED_FAILURES`] fail on purpose.

use std::path::{Path, PathBuf};
use std::rc::Rc;

use sha2::{Digest, Sha256};
use speedyspec_abilities_php::{AbilitiesArea, AbilitiesService};
use speedyspec_compat::wordpress_release;
use speedyspec_container::Container;
use speedyspec_container_php::ContainerArea;
use speedyspec_hooks_memory::MemoryHooks;
use speedyspec_operation_contracts::Identity;
use speedyspec_operation_php::{FixedSiteKey, OperationArea, PhpIdentity, SiteId};
use speedyspec_php::{BootOptions, NativeArea, Php};
use speedyspec_transport::MemoryBindingStore;
use speedyspec_wp_hooks_memory::MemoryHookStore;
use speedyspec_wp_hooks_php::WpHooksArea;

/// WordPress tests the shim fails on purpose, in the order they run: none. A new entry needs its reason here.
///
/// One copied test never runs, so it is neither counted nor listed: `test_execute_with_different_callbacks`. Its data
/// provider builds `array( $this, … )`, and the pinned PHPUnit 11 runs data providers statically, so PHPUnit reports
/// the provider invalid before the test is collected. That is the test runner's limit, not the shim's; the callback
/// kinds it covers (function names, closures, static methods) run in the suite's other tests.
const EXPECTED_FAILURES: &[&str] = &[];

/// Every copied test, data sets counted one by one.
const EXPECTED_TESTS: i64 = 201;

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
    let identity: Rc<dyn Identity> = Rc::new(PhpIdentity::new());
    let operations = Rc::new(OperationArea::new(
        container.clone(),
        identity.clone(),
        Rc::new(FixedSiteKey(SiteId {
            blog: 1,
            fingerprint: String::new(),
        })),
    ));
    let bindings = Rc::new(MemoryBindingStore::new());
    let service = Rc::new(AbilitiesService::new(
        container.clone(),
        operations.clone(),
        bindings.clone(),
        identity,
    ));
    let areas: Vec<Rc<dyn NativeArea>> = vec![
        speedyspec_php_ext::twins_area(),
        Rc::new(ContainerArea::new("test", container)),
        Rc::new(hooks),
        operations,
        Rc::new(AbilitiesArea::new(bindings).with_service(service)),
    ];
    let php = Php::boot(BootOptions {
        hooks: Rc::new(MemoryHooks::new()),
        preload: &[],
        areas,
    })
    .expect("PHP boots");
    let config = root().join("tests/php/phpunit-abilities.xml");
    let junit = Path::new(env!("CARGO_TARGET_TMPDIR")).join("wordpress-abilities.junit.xml");
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
        &format!(
            "wordpress abilities ({tests} tests, {} expected failures)",
            EXPECTED_FAILURES.len()
        ),
        // Every copied test runs: a test that stops being collected, or is skipped, is a change to look at.
        tests == EXPECTED_TESTS && total("skipped") == 0 && failed == EXPECTED_FAILURES,
        (&status, tests, failures, errors, &failed),
    );
}
