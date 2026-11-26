//! WordPress's kses functions in PHP, native over the Rust kses API (Facade, Adapter).
//!
//! [`KsesArea`] declares, at the start of every request (from `kses.php`), `wp_kses()` and every other function of
//! the release's `kses.php` this port covers, plus `wp_allowed_protocols()`, with WordPress's signatures. Each is one line
//! calling a native function under `SpeedySpec\WpKses\Native\`, which dispatches through the handler bound in the
//! container (resolved once), so PHP may override one. The release's own `kses.php` still runs for its globals
//! ([`prepare_release`] strips the functions); the adapters read those globals live, so a plugin that writes
//! `$allowedposttags` changes what kses allows, as in WordPress.

mod adapters;
mod native;
mod release;

use std::cell::RefCell;
use std::rc::Rc;

use speedyspec_container::Container;
use speedyspec_php::NativeArea;
use speedyspec_php::ext_php_rs::builders::FunctionBuilder;
use speedyspec_wp_hooks::HookApi;
use speedyspec_wp_kses::{KsesApi, KsesDeps, register_kses_handlers};

pub use adapters::{
    PhpAllowedGlobals, PhpEscaper, PhpHtmlApiSanitizer, PhpOperatingMode, PhpPolicy, PhpUploads,
    PhpValueCallbacks,
};
pub use release::{PORTED_FUNCTIONS, prepare_release, strip_ported};

/// Runs the kses scenarios through the native functions (the port side of `compat/kses`), over `release` prepared
/// next to it with [`prepare_release`].
pub fn kses_scenario_runner(
    php: speedyspec_php::Php,
    release: &std::path::Path,
) -> Result<speedyspec_compat::PhpRunner, String> {
    let out = release.with_file_name("wordpress-kses-port");
    let root = prepare_release(release, &out)?;
    let code = format!(
        "define('SPEEDYSPEC_COMPAT_WP_ROOT', {:?}); define('SPEEDYSPEC_COMPAT_PORT', true); require {:?};",
        format!("{}/", root.display()),
        speedyspec_compat::compat_dir()
            .join("kses/setup.php")
            .display()
            .to_string()
    );
    Ok(speedyspec_compat::PhpRunner::new(php, move |php| {
        php.eval("speedyspec://compat/kses-setup", &code)
            .map(|_| ())
            .map_err(|e| e.to_string())
    }))
}

/// The PHP declarations of [`PORTED_FUNCTIONS`], evaluated at the start of every request.
pub const FACADE: &str = include_str!("kses.php");

pub(crate) struct Shared {
    pub handlers: native::Handlers,
    pub api: Rc<KsesApi>,
    pub globals: Rc<PhpAllowedGlobals>,
}

thread_local! {
    pub(crate) static SHARED: RefCell<Option<Rc<Shared>>> = const { RefCell::new(None) };
}

/// The native area of WordPress's kses functions.
pub struct KsesArea {
    shared: Rc<Shared>,
}

impl KsesArea {
    /// The kses API over `hooks` with the PHP adapters, and its handlers registered in `container`.
    pub fn new(container: Rc<Container>, hooks: Rc<HookApi>) -> KsesArea {
        let globals = Rc::new(PhpAllowedGlobals::default());
        let api = KsesApi::shared(KsesDeps {
            hooks,
            escaper: Rc::new(PhpEscaper),
            policy: Rc::new(PhpPolicy),
            globals: globals.clone(),
            uploads: Rc::new(PhpUploads),
            callbacks: Rc::new(PhpValueCallbacks),
            html_api: Rc::new(PhpHtmlApiSanitizer),
            mode: Rc::new(PhpOperatingMode),
        });
        register_kses_handlers(&container, api.clone());
        let shared = Rc::new(Shared {
            handlers: native::Handlers::new(&container),
            api,
            globals,
        });
        SHARED.with(|s| *s.borrow_mut() = Some(shared.clone()));
        KsesArea { shared }
    }

    /// The kses API for Rust callers.
    pub fn api(&self) -> Rc<KsesApi> {
        self.shared.api.clone()
    }
}

impl NativeArea for KsesArea {
    fn functions(&self) -> Vec<FunctionBuilder<'static>> {
        native::functions()
    }

    fn request_start(&self) {
        self.shared.api.new_request();
        self.shared.globals.request_start();
        if let Err(err) = speedyspec_php::eval("speedyspec://wp-kses", FACADE) {
            eprintln!("speedyspec_wordpress: installing the kses functions failed: {err}");
        }
    }

    fn request_end(&self) {
        self.shared.globals.request_end();
        self.shared.api.new_request();
    }
}
