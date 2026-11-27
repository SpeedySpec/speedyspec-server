//! WordPress's post registries and post reading in PHP, native over the Rust post rules and read use cases (Facade,
//! Adapter).
//!
//! [`PostsArea`] declares, at the start of every request, the post functions of `post.php` this port covers with
//! WordPress's signatures:
//! - from `post-types.php`, the 20 registry functions (`register_post_type()` and its readers,
//!   `register_post_status()` and its readers, the supports functions, the label and capability helpers, and
//!   `create_initial_post_types()`);
//! - from `posts.php`, the 35 reading functions (`get_post()` and the readers beside it, the page functions, the
//!   counts and dates, the `posts` cache functions, `sanitize_post()` and `sanitize_post_field()`, the pings and
//!   `is_sticky()`).
//!
//! Each is one line calling a native function under `SpeedySpec\WpPosts\Native\`. The reading natives dispatch
//! through the use case's handler bound in the container (Command/CQRS), so PHP may override one.
//!
//! The area depends on contracts only: the Hook API, the object cache, `wpdb` and the metadata API come in as
//! `Rc<dyn …>` from the runtime. The registries are PHP's globals, read live during a request (Registry:
//! [`PhpPostTypes`], [`PhpPostStatuses`], [`PhpPostTypeSupports`]). `wp_posts` is a port resolved from the container
//! on every call ([`ResolvedRepository`]): the SQL table over `wpdb` is the default binding, and a PHP object
//! implementing the `SpeedySpec\WpPosts\PostRepository` twin, bound at [`POST_REPOSITORY`], replaces it. The other
//! outbound ports are PHP adapters ([`PhpCurrentPost`], [`PhpFormatting`], [`PhpUsers`], [`PhpTaxonomy`] and the
//! registries' [`PhpTranslator`], [`PhpRewriteContext`], [`PhpTitleSanitizer`]). `WP_Post` and `WP_Post_Type` stay
//! WordPress's classes.
//!
//! Patterns: Facade (the PHP functions), Adapter (the ports over PHP and its globals), Decorator ([`PhpValueHooks`],
//! [`PhpValueCache`]), Repository ([`ResolvedRepository`], [`PhpPostRepository`]), Registry (`$wp_post_types`,
//! `$wp_post_statuses` and `$_wp_post_type_features`, read live), Observer (the hooks, through [`HookApi`]), Command
//! (`create_initial_post_types()`' table of calls, and the handlers).

mod adapters;
mod codec;
mod hooks;
mod ports;
mod read;
mod registry;
mod release;
mod runner;
mod twins;

use std::cell::RefCell;
use std::rc::Rc;

use speedyspec_container::Container;
use speedyspec_container_php::ContainerArea;
use speedyspec_php::NativeArea;
use speedyspec_php::ext_php_rs::builders::FunctionBuilder;
use speedyspec_wp_db_contracts::Wpdb;
use speedyspec_wp_hooks_contracts::HookApi;
use speedyspec_wp_meta_contracts::Metadata;
use speedyspec_wp_object_cache_contracts::ObjectCache;
use speedyspec_wp_posts::{PostsApi, PostsDeps, register_posts_handlers};
use speedyspec_wp_posts_contracts::PostRepository;
use speedyspec_wp_posts_sql::SqlPostRepository;

pub use adapters::{
    PhpPostStatuses, PhpPostTypeSupports, PhpPostTypes, PhpRewriteContext, PhpTitleSanitizer,
    PhpTranslator,
};
pub use codec::{Thrown, copy_php, from_php, to_php};
pub use hooks::PhpValueHooks;
pub use ports::{
    POST_REPOSITORY, PhpCacheInvalidation, PhpCurrentPost, PhpFormatting, PhpOptions, PhpTaxonomy,
    PhpUsers, PhpValueCache, ResolvedRepository, SystemMicrotime,
};
pub use release::{PORTED_FUNCTIONS, prepare_release, strip_ported};
pub use runner::{php_string, posts_read_scenario_runner};
pub use twins::{PhpPostRepository, PostRepositoryTwin};

const FACADE: &str = include_str!("post-types.php");
const READING: &str = include_str!("posts.php");

thread_local! {
    /// The Hook API the native functions use, for the life of the worker.
    pub(crate) static SHARED: RefCell<Option<Rc<dyn HookApi>>> = const { RefCell::new(None) };
}

/// The native area of WordPress's post registries and post reading.
pub struct PostsArea {
    api: Rc<PostsApi>,
}

impl PostsArea {
    /// The registries, filtering and firing through `hooks`, and the read use cases over `wpdb`'s `default` engine,
    /// the request's object cache and the metadata API, with their handlers registered in `container` and the SQL
    /// table bound as the default post repository.
    pub fn new(
        container: Rc<Container>,
        hooks: Rc<dyn HookApi>,
        cache: Rc<dyn ObjectCache>,
        wpdb: Rc<dyn Wpdb>,
        metadata: Rc<dyn Metadata>,
    ) -> PostsArea {
        SHARED.with(|s| *s.borrow_mut() = Some(hooks.clone()));
        let sql: Rc<dyn PostRepository> = Rc::new(SqlPostRepository::new(wpdb, "default"));
        let default = sql.clone();
        container.bind_port::<dyn PostRepository>(POST_REPOSITORY, move |_, _| Ok(default.clone()));
        let current = Rc::new(PhpCurrentPost::new());
        let api = PostsApi::new(PostsDeps {
            hooks: Rc::new(PhpValueHooks::new(hooks.clone())),
            cache: Rc::new(PhpValueCache::new(cache)),
            repository: Rc::new(ResolvedRepository::new(&container, sql)),
            metadata,
            types: Rc::new(PhpPostTypes),
            statuses: Rc::new(PhpPostStatuses),
            current: current.clone(),
            formatting: Rc::new(PhpFormatting),
            users: Rc::new(PhpUsers),
            terms: Rc::new(PhpTaxonomy),
            clock: Rc::new(SystemMicrotime),
            invalidation: Rc::new(PhpCacheInvalidation),
            options: Rc::new(PhpOptions),
        });
        register_posts_handlers(&container, api.clone());
        let reading = Rc::new(read::Reading::new(&container, current));
        read::READING.with(|r| *r.borrow_mut() = Some(reading));
        PostsArea { api }
    }

    /// The post read API for Rust callers.
    pub fn api(&self) -> Rc<PostsApi> {
        self.api.clone()
    }

    /// `area` with the post repository registered as a port, so a PHP object implementing the
    /// `SpeedySpec\WpPosts\PostRepository` twin and bound at [`POST_REPOSITORY`] reaches the use cases.
    pub fn ports(area: ContainerArea) -> ContainerArea {
        area.port::<dyn PostRepository>(POST_REPOSITORY, twins::wrap)
    }
}

impl NativeArea for PostsArea {
    fn functions(&self) -> Vec<FunctionBuilder<'static>> {
        registry::functions()
    }

    fn request_start(&self) {
        codec::clear_live();
        for (label, source) in [
            ("speedyspec://wp-post-types", FACADE),
            ("speedyspec://wp-posts", READING),
        ] {
            if let Err(err) = speedyspec_php::eval(label, source) {
                eprintln!("speedyspec_wordpress: installing the post functions failed: {err}");
            }
        }
    }

    fn request_end(&self) {
        codec::clear_live();
        let _ = adapters::take_parked();
        registry::reset_depth();
    }
}
