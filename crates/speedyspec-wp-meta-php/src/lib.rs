//! WordPress's `meta.php` in PHP, native over the Rust metadata API (Facade, Adapter).
//!
//! [`MetaArea`] declares, at the start of every request, the 23 functions of `meta.php` this port covers (all but
//! `get_meta_sql()`) with WordPress's signatures, from `meta.php`, and `WP_Metadata_Lazyloader`, from
//! `lazyloader.php`. Each function is one line calling a native function under `SpeedySpec\WpMeta\Native\`, which
//! dispatches through the use case's handler bound in the container (Command/CQRS), so PHP may override one.
//!
//! The area depends on contracts only: the Hook API, the object cache and `wpdb` come in as `Rc<dyn …>` from the
//! runtime. The meta repositories are a port resolved from the container on every call ([`ResolvedRepositories`]):
//! the SQL tables over `wpdb` ([`SqlMetaRepositories`]) are the default binding, and a PHP object implementing the
//! `SpeedySpec\WpMeta\MetaRepositories` twin, bound at [`META_REPOSITORIES`], replaces them. The other outbound ports
//! are PHP adapters ([`PhpObjectSubtypes`], [`PhpInstalling`] and the others) until the seams that own posts, terms, comments, users and sites bind Rust ones.
//!
//! Patterns: Facade (the PHP functions), Adapter (the ports over PHP, [`PhpMetaRepositories`]), Decorator
//! ([`PhpValueHooks`], [`PhpTableNames`]), Command/CQRS (the handlers), Registry (`$wp_meta_keys`, read live by
//! [`PhpMetaKeys`]).

mod adapters;
mod args;
mod codec;
mod hooks;
mod lazy;
mod native;
mod register;
mod release;
mod twins;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use speedyspec_container::Container;
use speedyspec_container_php::ContainerArea;
use speedyspec_php::NativeArea;
use speedyspec_php::ext_php_rs::builders::FunctionBuilder;
use speedyspec_wp_db_contracts::Wpdb;
use speedyspec_wp_hooks_contracts::HookApi;
use speedyspec_wp_meta::{
    BlogSubtypes, LazyLoader, MetaDeps, MetadataApi, SubtypesByType, UnfilteredProtection,
    register_meta_handlers,
};
use speedyspec_wp_meta_contracts::{MetaRepositories, ObjectSubtypes};
use speedyspec_wp_meta_sql::SqlMetaRepositories;
use speedyspec_wp_object_cache_contracts::ObjectCache;

pub use adapters::{
    META_REPOSITORIES, PhpCallables, PhpDefaultSchemaCheck, PhpInstalling, PhpMetaKeys,
    PhpObjectSubtypes, PhpRevisionSupport, PhpSites, PhpTableNames, ResolvedRepositories,
};
pub use codec::{Thrown, from_php, from_php_live, to_php};
pub use hooks::PhpValueHooks;
pub use release::{
    LAZYLOADER_FILE, LAZYLOADER_STUB, PORTED_FUNCTIONS, prepare_release, strip_ported,
};
pub use twins::{MetaRepositoriesTwin, MetaRepositoryTwin, PhpMetaRepositories, PhpMetaRepository};

const FACADE: &str = include_str!("meta.php");
const LAZYLOADER: &str = include_str!("lazyloader.php");

/// What the native functions share for the life of the worker.
pub(crate) struct Shared {
    pub(crate) handlers: native::Handlers,
    pub(crate) api: Rc<MetadataApi>,
    /// The Hook API as PHP gave it: `register_meta()` passes callbacks through it unconverted.
    pub(crate) raw_hooks: Rc<dyn HookApi>,
    /// The Hook API the use cases see.
    pub(crate) hooks: Rc<dyn HookApi>,
    /// `wpdb` with the dynamic table properties, for `_get_meta_table()`.
    pub(crate) tables: Rc<PhpTableNames>,
    /// The request's lazy loaders, one per `WP_Metadata_Lazyloader`.
    pub(crate) loaders: RefCell<Vec<Rc<LazyLoader>>>,
}

thread_local! {
    pub(crate) static SHARED: RefCell<Option<Rc<Shared>>> = const { RefCell::new(None) };
}

/// The native area of WordPress's meta functions.
pub struct MetaArea {
    api: Rc<MetadataApi>,
    shared: Rc<Shared>,
}

impl MetaArea {
    /// The metadata API over `wpdb`'s `default` engine and the request's object cache, filtering through `hooks`,
    /// with its handlers registered in `container` and the SQL tables bound as the default meta repositories.
    pub fn new(
        container: Rc<Container>,
        hooks: Rc<dyn HookApi>,
        cache: Rc<dyn ObjectCache>,
        wpdb: Rc<dyn Wpdb>,
    ) -> MetaArea {
        let tables = Rc::new(PhpTableNames::new(wpdb));
        let sql: Rc<dyn MetaRepositories> =
            Rc::new(SqlMetaRepositories::new(tables.clone(), "default"));
        let default = sql.clone();
        container
            .bind_port::<dyn MetaRepositories>(META_REPOSITORIES, move |_, _| Ok(default.clone()));
        let php_subtypes: Rc<dyn ObjectSubtypes> = Rc::new(PhpObjectSubtypes);
        let subtypes = ["post", "term", "comment", "user"]
            .into_iter()
            .fold(SubtypesByType::new(), |s, t| {
                s.bind(t, php_subtypes.clone())
            })
            .bind("blog", Rc::new(BlogSubtypes::new(Rc::new(PhpSites))));
        let converted: Rc<dyn HookApi> = Rc::new(PhpValueHooks::new(hooks.clone()));
        let api = MetadataApi::new(MetaDeps {
            hooks: converted.clone(),
            cache,
            repositories: Rc::new(ResolvedRepositories::new(&container, sql)),
            keys: Rc::new(PhpMetaKeys),
            subtypes: Rc::new(subtypes),
            // The unfiltered rule: the use cases apply the `is_protected_meta` filter once over it.
            protection: Rc::new(UnfilteredProtection),
            installing: Rc::new(PhpInstalling),
        });
        register_meta_handlers(&container, api.clone());
        let shared = Rc::new(Shared {
            handlers: native::Handlers::new(&container),
            api: api.clone(),
            raw_hooks: hooks,
            hooks: converted,
            tables,
            loaders: RefCell::new(Vec::new()),
        });
        SHARED.with(|s| *s.borrow_mut() = Some(shared.clone()));
        MetaArea { api, shared }
    }

    /// The metadata API for Rust callers.
    pub fn api(&self) -> Rc<MetadataApi> {
        self.api.clone()
    }

    /// `area` with the meta repositories registered as a port, so a PHP object implementing the
    /// `SpeedySpec\WpMeta\MetaRepositories` twin and bound at [`META_REPOSITORIES`] reaches the use cases.
    pub fn ports(area: ContainerArea) -> ContainerArea {
        area.port::<dyn MetaRepositories>(META_REPOSITORIES, |object| {
            Rc::new(PhpMetaRepositories::new(object))
        })
    }
}

impl NativeArea for MetaArea {
    fn functions(&self) -> Vec<FunctionBuilder<'static>> {
        native::functions()
    }

    fn request_start(&self) {
        self.shared.loaders.borrow_mut().clear();
        codec::clear_live();
        for (label, source) in [
            ("speedyspec://wp-meta-lazyloader", LAZYLOADER),
            ("speedyspec://wp-meta", FACADE),
        ] {
            if let Err(err) = speedyspec_php::eval(label, source) {
                eprintln!("speedyspec_wordpress: installing the meta functions failed: {err}");
            }
        }
    }

    fn request_end(&self) {
        self.shared.loaders.borrow_mut().clear();
        codec::clear_live();
        let _ = adapters::take_parked();
        native::reset_depth();
    }
}

/// Runs the metadata scenarios through the native functions (the port side of `compat/metadata`), over `release`
/// prepared next to it with [`prepare_release`]. Each scenario's request is a multisite install while `multisite`
/// holds true (the scenarios of the `-multisite` fixture), as the recorder's is.
pub fn metadata_scenario_runner(
    php: speedyspec_php::Php,
    release: &std::path::Path,
    multisite: Rc<Cell<bool>>,
) -> Result<speedyspec_compat::PhpRunner, String> {
    let out = release.with_file_name("wordpress-metadata-port");
    let root = prepare_release(release, &out)?;
    let code = |multisite: bool| {
        format!(
            "define('SPEEDYSPEC_COMPAT_WP_ROOT', {:?}); define('SPEEDYSPEC_COMPAT_PORT', true); \
             define('SPEEDYSPEC_COMPAT_MULTISITE', {multisite}); require {:?};",
            format!("{}/", root.display()),
            speedyspec_compat::compat_dir()
                .join("metadata/setup.php")
                .display()
                .to_string()
        )
    };
    let (single, network) = (code(false), code(true));
    Ok(speedyspec_compat::PhpRunner::new(php, move |php| {
        let code = if multisite.get() { &network } else { &single };
        php.eval("speedyspec://compat/metadata-setup", code)
            .map(|_| ())
            .map_err(|e| e.to_string())
    }))
}
