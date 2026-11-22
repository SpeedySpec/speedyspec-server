//! WordPress's own diff tests (tests/php/wordpress/diff, copied from wordpress-develop) pass with the native
//! `wp_text_diff()` loaded, run by the pinned PHPUnit inside the embedded PHP. They test the Text_Diff classes,
//! which stay PHP; the bootstrap also checks that the native and legacy paths of `wp_text_diff()` give one table.

use std::path::{Path, PathBuf};
use std::rc::Rc;

use sha2::{Digest, Sha256};
use speedyspec_compat::wordpress_release;
use speedyspec_container::Container;
use speedyspec_container_php::ContainerArea;
use speedyspec_hooks_memory::MemoryHooks;
use speedyspec_php::{BootOptions, NativeArea, Php};
use speedyspec_wp_hooks_memory::MemoryHookStore;
use speedyspec_wp_hooks_php::WpHooksArea;
use speedyspec_wp_text_diff_php::TextDiffArea;

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
    let release = wordpress_release("7.1.5").expect("the pinned WordPress release");
    // SAFETY: set before PHP boots, while this process has one thread.
    unsafe { std::env::set_var("SPEEDYSPEC_WP_ROOT", &release) };
    let phar = phpunit();
    let container = Rc::new(Container::new());
    let hooks = WpHooksArea::new(container.clone(), Rc::new(MemoryHookStore::new())).with_testing();
    let text_diff = TextDiffArea::new(hooks.api(), container.clone());
    text_diff.bind_defaults();
    let container_area = TextDiffArea::ports(ContainerArea::new("test", container));
    let areas: Vec<Rc<dyn NativeArea>> = vec![
        speedyspec_php_ext::twins_area(),
        Rc::new(container_area),
        Rc::new(hooks),
        Rc::new(text_diff),
    ];
    let php = Php::boot(BootOptions {
        hooks: Rc::new(MemoryHooks::new()),
        preload: &[],
        areas,
    })
    .expect("PHP boots");
    let config = root().join("tests/php/phpunit-text-diff.xml");
    let junit = Path::new(env!("CARGO_TARGET_TMPDIR")).join("wordpress-text-diff.junit.xml");
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
        "wordpress_diff_tests_pass",
        matches!(status, Ok(0)) && tests >= 26 && failures == 0 && errors == 0,
        (&status, tests, failures, errors),
    );
}
