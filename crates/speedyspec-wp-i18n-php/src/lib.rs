//! WordPress's translation functions in PHP over the Rust translations port (Facade, Adapter, Proxy).
//!
//! [`I18nArea`] registers, once at module startup, native `translate()`, `translate_with_gettext_context()`,
//! `__()`, `_x()`, `_n()` and `_nx()` under WordPress's names. Each serves the common call itself: the domain's
//! entry in `$l10n` is a `WP_Translations` over the request's controller and nothing is hooked to the call's
//! filters. Every other call goes to WordPress 7.1.5's own PHP function, which the facade (`i18n.php`) keeps under
//! `SpeedySpec\WpI18n\Legacy\`, so the result is WordPress's in every case.
//!
//! The facade also declares `WP_Translation_Controller` and the `WP_Translation_File` classes as shells over the
//! Rust controller, and `WP_Translations` as WordPress has it. Translation files are read once per worker and kept
//! across requests (`MemoryTranslationFileCache`).

mod effects;
mod hot;
mod internal;
mod reader;
mod release;
mod state;
mod zv;

use std::rc::Rc;

use speedyspec_container::Container;
use speedyspec_php::NativeArea;
use speedyspec_php::ext_php_rs::builders::FunctionBuilder;
use speedyspec_wp_hooks::{HookApi, HookStore};
use speedyspec_wp_hooks_php::HOOK_STORE;
use speedyspec_wp_i18n::I18nApi;

pub use reader::PhpFallbackReader;
pub use release::{PORTED_FUNCTIONS, prepare_release, strip_ported};

const FACADE: &str = include_str!("i18n.php");

/// The native area of WordPress's translation functions.
pub struct I18nArea {
    shared: Rc<state::Shared>,
    container: Rc<Container>,
}

impl I18nArea {
    /// The area over `hooks` (whose store is bound in `container`, as `WpHooksArea` binds it), with a translation
    /// file cache that lives as long as the worker.
    pub fn new(container: Rc<Container>, hooks: Rc<HookApi>) -> I18nArea {
        let shared = Rc::new(state::Shared::new(hooks));
        state::install(shared.clone());
        I18nArea { shared, container }
    }

    /// The translation API for Rust callers.
    pub fn api(&self) -> Rc<I18nApi> {
        self.shared.api.clone()
    }
}

impl NativeArea for I18nArea {
    fn functions(&self) -> Vec<FunctionBuilder<'static>> {
        let mut f = hot::functions();
        f.extend(internal::functions());
        f.extend(internal::wordpress_functions());
        f
    }

    fn request_start(&self) {
        self.shared.start_request();
        hot::start_request();
        // The Hook API resolves its store from the container on every access; the hot path resolves it once.
        self.shared
            .api
            .set_request_store(self.container.make_port::<dyn HookStore>(HOOK_STORE).ok());
        if let Err(err) = speedyspec_php::eval("speedyspec://wp-i18n", FACADE) {
            eprintln!("speedyspec_wordpress: installing the translation functions failed: {err}");
        }
    }

    fn request_end(&self) {
        self.shared.api.set_request_store(None);
        self.shared.end_request();
    }
}
