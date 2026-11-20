//! WordPress's option functions in PHP, native over the Rust options API (Facade, Adapter).
//!
//! [`OptionsArea`] declares, at the start of every request (from `options.php`), `get_option()` and the other
//! functions of `option.php` this port covers, with WordPress's signatures; each is one line calling a native
//! function under `SpeedySpec\WpOptions\Native\`, which dispatches through the handler bound in the container
//! (resolved once), so PHP may override one. The options themselves are read and written through
//! `SqlOptionRepository` on the global `wpdb`, so the `query` filter fires and `$wpdb->num_queries` counts.
//!
//! An array or object given as `get_option`'s, `add_option`'s or `update_option`'s name throws the `TypeError`
//! WordPress's own `isset( $deprecated_keys[ $option ] )` throws.

mod adapters;
mod codec;
mod native;
mod release;

use std::cell::RefCell;
use std::rc::Rc;

use speedyspec_container::Container;
use speedyspec_php::NativeArea;
use speedyspec_php::ext_php_rs::builders::FunctionBuilder;
use speedyspec_wp_db::WpDb;
use speedyspec_wp_hooks::HookApi;
use speedyspec_wp_object_cache::CurrentObjectCache;
use speedyspec_wp_options::{
    OptionsApi, OptionsDeps, WordPressAutoloadPolicy, register_options_handlers,
};
use speedyspec_wp_options_sql::SqlOptionRepository;

pub use adapters::{
    PhpDeprecations, PhpInstalling, PhpMultisite, PhpOptionGroups, PhpProtectedOptions,
    PhpSanitizing, Snapshot,
};
pub use codec::PhpCodec;
pub use release::{PORTED_FUNCTIONS, prepare_release, strip_ported};

/// Runs the options scenarios through the native functions (the port side of `compat/options`), over `release`
/// prepared next to it with `prepare_release`.
pub fn options_scenario_runner(
    php: speedyspec_php::Php,
    release: &std::path::Path,
) -> Result<speedyspec_compat::PhpRunner, String> {
    let out = release.with_file_name("wordpress-options-port");
    let root = prepare_release(release, &out)?;
    let code = format!(
        "define('SPEEDYSPEC_COMPAT_WP_ROOT', {:?}); define('SPEEDYSPEC_COMPAT_PORT', true); require {:?};",
        format!("{}/", root.display()),
        speedyspec_compat::compat_dir()
            .join("options/setup.php")
            .display()
            .to_string()
    );
    Ok(speedyspec_compat::PhpRunner::new(php, move |php| {
        php.eval("speedyspec://compat/options-setup", &code)
            .map(|_| ())
            .map_err(|e| e.to_string())
    }))
}

const FACADE: &str = include_str!("options.php");

pub(crate) struct Shared {
    pub handlers: native::Handlers,
    pub snapshot: Rc<Snapshot>,
}

thread_local! {
    pub(crate) static SHARED: RefCell<Option<Rc<Shared>>> = const { RefCell::new(None) };
}

/// The native area of WordPress's option functions.
pub struct OptionsArea {
    api: Rc<OptionsApi>,
    shared: Rc<Shared>,
}

impl OptionsArea {
    /// The options API over `wp_options` on `wpdb`'s `default` engine, with the PHP adapters, and its handlers
    /// registered in `container`.
    pub fn new(
        container: Rc<Container>,
        hooks: Rc<HookApi>,
        cache: Rc<CurrentObjectCache>,
        wpdb: Rc<WpDb>,
    ) -> OptionsArea {
        let snapshot = Rc::new(Snapshot::default());
        let repository = Rc::new(SqlOptionRepository::new(wpdb, "default".into()));
        let deps = OptionsDeps {
            autoload: Rc::new(WordPressAutoloadPolicy::new(hooks.clone())),
            codec: Rc::new(PhpCodec),
            installing: Rc::new(PhpInstalling(snapshot.clone())),
            multisite: Rc::new(PhpMultisite(snapshot.clone())),
            sanitizing: Rc::new(PhpSanitizing::new(hooks.clone())),
            deprecations: Rc::new(PhpDeprecations),
            protected: Rc::new(PhpProtectedOptions),
            groups: Rc::new(PhpOptionGroups),
            hooks,
            cache,
            repository,
        };
        let api = Rc::new(OptionsApi::new(deps));
        register_options_handlers(&container, api.clone());
        let shared = Rc::new(Shared {
            handlers: native::Handlers::new(&container),
            snapshot,
        });
        SHARED.with(|s| *s.borrow_mut() = Some(shared.clone()));
        OptionsArea { api, shared }
    }

    /// The options API for Rust callers.
    pub fn api(&self) -> Rc<OptionsApi> {
        self.api.clone()
    }
}

impl NativeArea for OptionsArea {
    fn functions(&self) -> Vec<FunctionBuilder<'static>> {
        native::functions()
    }

    fn request_start(&self) {
        self.shared.snapshot.clear();
        if let Err(err) = speedyspec_php::eval("speedyspec://wp-options", FACADE) {
            eprintln!("speedyspec_wordpress: installing the option functions failed: {err}");
        }
    }

    fn request_end(&self) {
        self.shared.snapshot.clear();
    }
}
