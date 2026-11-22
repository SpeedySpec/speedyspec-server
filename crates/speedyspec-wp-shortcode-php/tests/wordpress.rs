//! WordPress's own shortcode tests (tests/php/wordpress/shortcode, copied from wordpress-develop) pass against the
//! native Shortcode API, run by the pinned PHPUnit inside the embedded PHP, with the native kses functions.
//! WordPress's legacy `shortcodes.php` is never loaded, and its `kses.php` runs only for the globals. Tests marked `@group documented-break` or `@group needs-wordpress-core` are excluded.

use std::path::{Path, PathBuf};
use std::rc::Rc;

use sha2::{Digest, Sha256};
use speedyspec_compat::wordpress_release;
use speedyspec_container::Container;
use speedyspec_container_php::ContainerArea;
use speedyspec_hooks_memory::MemoryHooks;
use speedyspec_php::{BootOptions, NativeArea, Php};
use speedyspec_php_ext::twins_area;
use speedyspec_wp_hooks_memory::MemoryHookStore;
use speedyspec_wp_hooks_php::WpHooksArea;
use speedyspec_wp_kses_php::{KsesArea, prepare_release};
use speedyspec_wp_shortcode_php::ShortcodeArea;

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
    let pin: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(root().join("tests/php/phpunit.json")).unwrap()).unwrap();
    let (url, sha256, version) =
        (pin["url"].as_str().unwrap(), pin["sha256"].as_str().unwrap(), pin["version"].as_str().unwrap());
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
    // The native kses functions replace the release's, so its kses.php runs only for the globals.
    let prepared = prepare_release(&release, &release.with_file_name("wordpress-shortcode-port"))
        .expect("the release prepared for kses");
    // SAFETY: set before PHP boots, while this process has one thread.
    unsafe { std::env::set_var("SPEEDYSPEC_WP_ROOT", &prepared) };
    let phar = phpunit();
    let container = Rc::new(Container::new());
    let hooks = WpHooksArea::new(container.clone(), Rc::new(MemoryHookStore::new())).with_testing();
    let kses = KsesArea::new(container.clone(), hooks.api());
    let shortcodes = ShortcodeArea::new(container.clone(), hooks.api(), kses.api());
    let areas: Vec<Rc<dyn NativeArea>> = vec![
        twins_area(),
        Rc::new(ContainerArea::new("tests", container)),
        Rc::new(hooks),
        Rc::new(kses),
        Rc::new(shortcodes),
    ];
    let php = Php::boot(BootOptions { hooks: Rc::new(MemoryHooks::new()), preload: &[], areas }).expect("PHP boots");
    let config = root().join("tests/php/phpunit-shortcode.xml");
    let junit = Path::new(env!("CARGO_TARGET_TMPDIR")).join("wordpress-shortcode.junit.xml");
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
        "wordpress_shortcode_tests_pass",
        matches!(status, Ok(0)) && tests >= 80 && failures == 0 && errors == 0,
        (&status, tests, failures, errors),
    );
}
