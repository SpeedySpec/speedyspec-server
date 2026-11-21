//! The port's side of the recorded rewrite scenarios (`compat/rewrite`).

use std::path::Path;

use speedyspec_compat::{PhpRunner, compat_dir};
use speedyspec_php::Php;

/// Runs each scenario in a fresh request with `compat/rewrite/setup.php` on the port's side: the release's
/// `class-wp.php` patched to resolve through the Rust router, over the native hooks and rewrite API.
pub fn rewrite_scenario_runner(php: Php, release: &Path) -> PhpRunner {
    let code = format!(
        "define('SPEEDYSPEC_COMPAT_WP_ROOT', {:?}); define('SPEEDYSPEC_COMPAT_PORT', true); require {:?};",
        format!("{}/", release.display()),
        compat_dir().join("rewrite/setup.php").display().to_string()
    );
    PhpRunner::new(php, move |php: &Php| {
        php.eval("speedyspec://compat/setup", &code)
            .map(|_| ())
            .map_err(|e| e.to_string())
    })
}
