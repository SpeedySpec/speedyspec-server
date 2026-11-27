//! The port side of `compat/posts-read`: the recorded scenarios run through the native functions.

use std::cell::Cell;
use std::path::Path;
use std::rc::Rc;

/// Runs the post reading scenarios through the native functions (the port side of `compat/posts-read`), over `root`,
/// a release already prepared for every area the PHP runs with: [`prepare_release`](crate::prepare_release), after
/// `speedyspec_wp_meta_php::prepare_release` when the native meta functions run too. Each scenario's request is a
/// multisite install while `multisite` holds true (the scenarios of the `-multisite` fixture), as the recorder's is.
///
/// compat/posts-read/setup.php seeds its tables on every request, so the PHP must run on a database of its own.
pub fn posts_read_scenario_runner(
    php: speedyspec_php::Php,
    root: &Path,
    multisite: Rc<Cell<bool>>,
) -> speedyspec_compat::PhpRunner {
    let code = |multisite: bool| {
        format!(
            "define('SPEEDYSPEC_COMPAT_WP_ROOT', {}); define('SPEEDYSPEC_COMPAT_PORT', true); \
             define('SPEEDYSPEC_COMPAT_MULTISITE', {multisite}); require {};",
            php_string(&format!("{}/", root.display())),
            php_string(
                &speedyspec_compat::compat_dir()
                    .join("posts-read/setup.php")
                    .display()
                    .to_string()
            )
        )
    };
    let (single, network) = (code(false), code(true));
    speedyspec_compat::PhpRunner::new(php, move |php| {
        let code = if multisite.get() { &network } else { &single };
        php.eval("speedyspec://compat/posts-read-setup", code)
            .map(|_| ())
            .map_err(|e| e.to_string())
    })
}

/// `text` as a single-quoted PHP string literal, so nothing in a path is interpolated.
pub fn php_string(text: &str) -> String {
    format!("'{}'", text.replace('\\', "\\\\").replace('\'', "\\'"))
}

#[cfg(test)]
mod tests {
    #[test]
    fn paths_are_single_quoted() {
        assert_eq!(super::php_string(r"/a/$b/c'd\e"), r"'/a/$b/c\'d\\e'");
    }
}
