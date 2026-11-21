//! WordPress's shortcode functions in PHP, native over the Rust Shortcode API (Facade, Adapter).
//!
//! [`ShortcodeArea`] declares, at the start of every request (from `shortcodes.php`), every function of WordPress's
//! `shortcodes.php` with WordPress's signatures; each is one line calling a native function under
//! `SpeedySpec\WpShortcode\Native\`, which dispatches through the handler bound in the container (resolved once),
//! so PHP may override one.
//!
//! - The registry is the `$shortcode_tags` global itself ([`PhpGlobalRegistry`]).
//! - PHP callables run as handlers through [`PhpShortcodeHandler`]; Rust handlers are declared as native functions
//!   with [`ShortcodeArea::register_rust_handler`], and the global holds their names.
//! - Notices, kses and `wp_html_split()` go to WordPress's own PHP until their ports land ([`legacy`]).

mod handler;
pub mod legacy;
mod native;
mod registry;

use std::rc::Rc;

use speedyspec_container::Container;
use speedyspec_php::NativeArea;
use speedyspec_php::ext_php_rs::builders::FunctionBuilder;
use speedyspec_wp_hooks::HookApi;
use speedyspec_wp_shortcode::{
    HandAttsParser, HandScanner, HookApiFilters, ShortcodeApi, ShortcodeDeps, register_shortcode_handlers,
};
use speedyspec_wp_shortcode_contracts::{AttsParser, ShortcodeHandler, ShortcodeScanner};

pub use handler::PhpShortcodeHandler;
pub use registry::PhpGlobalRegistry;

use handler::{NamedRustHandler, RustHandlers};
use legacy::{PhpDiagnostics, PhpHtmlSplitter, PhpKsesAttributes, PhpStringConversion};

const FACADE: &str = include_str!("shortcodes.php");

/// Runs the shortcode scenarios through the native functions (the port side of `compat/shortcode`), loading
/// WordPress's formatting and kses files from `release`.
pub fn shortcode_scenario_runner(php: speedyspec_php::Php, release: &std::path::Path) -> speedyspec_compat::PhpRunner {
    let code = format!(
        "define('SPEEDYSPEC_COMPAT_WP_ROOT', {:?}); define('SPEEDYSPEC_COMPAT_PORT', true); require {:?};",
        format!("{}/", release.display()),
        speedyspec_compat::compat_dir().join("shortcode/setup.php").display().to_string()
    );
    speedyspec_compat::PhpRunner::new(php, move |php| {
        php.eval("speedyspec://compat/shortcode-setup", &code).map(|_| ()).map_err(|e| e.to_string())
    })
}

/// The native area of WordPress's shortcode functions.
pub struct ShortcodeArea {
    api: Rc<ShortcodeApi>,
    handlers: Rc<native::Handlers>,
    rust: Rc<RustHandlers>,
}

impl ShortcodeArea {
    /// The Shortcode API over `$shortcode_tags`, with the Hook API's filters and the PHP adapters, and its
    /// handlers registered in `container`.
    pub fn new(container: Rc<Container>, hooks: Rc<HookApi>) -> Self {
        Self::with_strategies(container, hooks, Rc::new(HandScanner::new()), Rc::new(HandAttsParser::new()))
    }

    /// As [`ShortcodeArea::new`], with the scanner and attribute parser given.
    pub fn with_strategies(
        container: Rc<Container>,
        hooks: Rc<HookApi>,
        scanner: Rc<dyn ShortcodeScanner>,
        atts: Rc<dyn AttsParser>,
    ) -> Self {
        let rust = Rc::new(RustHandlers::default());
        let api = Rc::new(ShortcodeApi::new(ShortcodeDeps {
            registry: Rc::new(PhpGlobalRegistry::new(rust.clone())),
            scanner,
            atts,
            filters: Rc::new(HookApiFilters::new(hooks)),
            diagnostics: Rc::new(PhpDiagnostics),
            html: Rc::new(PhpHtmlSplitter),
            kses: Rc::new(PhpKsesAttributes),
            strings: Rc::new(PhpStringConversion),
        }));
        register_shortcode_handlers(&container, api.clone());
        Self { api, handlers: Rc::new(native::Handlers::new(&container)), rust }
    }

    /// Declares the native PHP function `php_name` as `handler`, so `add_shortcode( $tag, $php_name )` runs it in
    /// Rust while `$shortcode_tags` holds the name, as WordPress's does. Call it before PHP boots.
    pub fn register_rust_handler(&self, php_name: &str, handler: Rc<dyn ShortcodeHandler>) {
        self.rust.0.borrow_mut().insert(
            php_name.to_ascii_lowercase(),
            Rc::new(NamedRustHandler { name: php_name.to_owned(), inner: handler }),
        );
    }

    /// The Shortcode API for Rust callers.
    pub fn api(&self) -> Rc<ShortcodeApi> {
        self.api.clone()
    }
}

impl NativeArea for ShortcodeArea {
    fn functions(&self) -> Vec<FunctionBuilder<'static>> {
        let mut functions = native::functions(self.handlers.clone());
        functions.extend(native::rust_handler_functions(&self.rust));
        functions
    }

    fn request_start(&self) {
        if let Err(err) = speedyspec_php::eval("speedyspec://wp-shortcode", FACADE) {
            eprintln!("speedyspec_wordpress: installing the shortcode functions failed: {err}");
        }
    }
}
