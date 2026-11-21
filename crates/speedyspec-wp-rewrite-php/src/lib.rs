//! WordPress's rewrite API in PHP, over the Rust port.
//!
//! [`RewriteArea`] declares, at the start of every request, the `EP_*` constants, the `WP_Rewrite` class (every
//! public property WordPress declares, with native methods), `WP_MatchesMapRegex`, and `rewrite.php`'s functions, so
//! plugins and themes see WordPress's whole rewrite API. WordPress creates the `$wp_rewrite` global itself.

mod adapters;
mod compat;
mod native;
mod state;
mod values;

use std::rc::Rc;

use speedyspec_container::Container;
use speedyspec_php::NativeArea;
use speedyspec_php::ext_php_rs::builders::FunctionBuilder;
use speedyspec_router::TableCache;
use speedyspec_router::dispatch::ChunkedAlternation;
use speedyspec_router_php::BoundRouteTableStore;
use speedyspec_router_store_memory::MemoryRouteTableStore;
use speedyspec_wp_hooks::HookApi;
use speedyspec_wp_rewrite::{
    RewriteDeps, RewriteOptions, RewriteService, WpRouting, register_rewrite_handlers,
};

pub use adapters::{
    HookWpLoaded, PhpMultisite, PhpPageByPath, PhpPageUris, PhpPostTypes, PhpQueryVars,
    PhpRewriteOptions, PhpSiteUrls,
};
pub use compat::rewrite_scenario_runner;
pub use state::{PhpGlobalRewriteState, PhpRewriteState};

/// The facade's PHP source.
const FACADE: &str = include_str!("rewrite.php");

/// The native area of the rewrite API.
pub struct RewriteArea {
    service: Rc<RewriteService>,
}

impl RewriteArea {
    /// The rewrite API over `options` (lazy mode: `WP::parse_request` resolves through `speedyspec_rewrite_match()`,
    /// whose table is cached in the route table store bound in `container`), over `options` (the options port in a
    /// worker, memory options in tests), firing hooks through `hooks`, with its handlers bound in `container`.
    pub fn new(
        container: Rc<Container>,
        hooks: Rc<HookApi>,
        options: Rc<dyn RewriteOptions>,
    ) -> RewriteArea {
        let store = Rc::new(BoundRouteTableStore::new(
            container.clone(),
            Rc::new(MemoryRouteTableStore::new()),
        ));
        let cache = Rc::new(TableCache::new(
            store,
            Rc::new(ChunkedAlternation::default()),
        ));
        let routing = Rc::new(WpRouting::new(options.clone(), cache));
        let deps = RewriteDeps {
            options,
            urls: Rc::new(PhpSiteUrls),
            multisite: Rc::new(PhpMultisite),
            loaded: Rc::new(HookWpLoaded::new(hooks.clone())),
            pages: Rc::new(PhpPageUris),
            post_types: Rc::new(PhpPostTypes),
            query_vars: Rc::new(PhpQueryVars),
            hooks,
            state: Rc::new(PhpGlobalRewriteState),
        };
        let service = Rc::new(RewriteService::new(deps));
        register_rewrite_handlers(&container, service.clone());
        native::install(container, service.clone(), routing);
        RewriteArea { service }
    }

    /// The service, for Rust callers in the same worker.
    pub fn service(&self) -> Rc<RewriteService> {
        self.service.clone()
    }
}

impl NativeArea for RewriteArea {
    fn functions(&self) -> Vec<FunctionBuilder<'static>> {
        native::functions()
    }

    fn request_start(&self) {
        if let Err(err) = speedyspec_php::eval("speedyspec://wp-rewrite", FACADE) {
            eprintln!("speedyspec_wordpress: installing the rewrite API failed: {err}");
        }
    }
}
