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
//!   `is_sticky()`);
//! - from `post-writes.php`, the 8 writing functions (`wp_insert_post()`, `wp_update_post()`,
//!   `wp_transition_post_status()`, `set_post_type()`, and the default callbacks WordPress registers by name:
//!   `_transition_post_status()`, `_publish_post_hook()`, `_future_post_hook()` and
//!   `wp_check_post_hierarchy_for_loops()`).
//!
//! Each is one line calling a native function under `SpeedySpec\WpPosts\Native\`. The reading and writing natives
//! dispatch through the use case's handler bound in the container (Command/CQRS), so PHP may override one.
//!
//! The write use cases reach unported code through container ports ([`Resolved`]) whose default bindings are PHP
//! adapters calling WordPress's own functions (users, taxonomy, comments, rewrite, uploads, `WP_Query`, the theme, the
//! column charsets), and cron through the Rust adapter over the cron scheduling core the runtime passes in. PHP
//! replaces any of them by binding an object implementing the port's twin (`SpeedySpec\WpPosts\<Port>`). The PHP
//! edge (the clock, `wp_truncate_slug()`, `sanitize_title()`, the `WP_Post` handles, `$wp_rewrite`'s slug settings,
//! `defined()` and `_doing_it_wrong()`) goes through plain adapters. Options go through the options contracts.
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
mod port_twins;
mod ports;
mod read;
mod registry;
mod release;
mod runner;
mod twins;
mod write;
mod write_ports;

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
use speedyspec_wp_options_contracts::Options;
use speedyspec_wp_posts::{
    PostWritesApi, PostsApi, PostsDeps, Scheduling, WriteDeps, register_post_write_handlers,
    register_posts_handlers,
};
use speedyspec_wp_posts_contracts as c;
use speedyspec_wp_posts_contracts::{
    Charset, Comments, PostQuery, PostRepository, Rewrite, Theme, Uploads,
};
use speedyspec_wp_posts_sql::SqlPostRepository;

pub use adapters::{
    PhpPostStatuses, PhpPostTypeSupports, PhpPostTypes, PhpRewriteContext, PhpTitleSanitizer,
    PhpTranslator,
};
pub use codec::{Thrown, copy_php, from_php, to_php};
pub use hooks::PhpValueHooks;
pub use port_twins::{
    CharsetTwin, CommentsTwin, PhpBound, PostQueryTwin, RewriteTwin, SchedulingTwin, TaxonomyTwin,
    ThemeTwin, UploadsTwin, UsersTwin,
};
pub use ports::{
    POST_REPOSITORY, PhpCacheInvalidation, PhpCurrentPost, PhpFormatting, PhpMicrotime,
    PhpTaxonomy, PhpUsers, PhpValueCache, ResolvedRepository, SystemMicrotime,
};
pub use release::{
    CLASS_FILE, GET_INSTANCE_BODY, PORTED_FUNCTIONS, prepare_release, strip_ported,
    swap_get_instance,
};
pub use runner::{php_string, posts_read_scenario_runner, posts_write_scenario_runner};
pub use twins::{PhpPostRepository, PostRepositoryTwin};
pub use write_ports::{
    CHARSET, COMMENTS, POST_QUERY, PhpCharset, PhpClock, PhpComments, PhpPostObjects, PhpPostQuery,
    PhpRewrite, PhpRuntime, PhpScheduling, PhpSlugSettings, PhpSlugTruncation, PhpTheme,
    PhpUploads, PhpWriteFormatting, REWRITE, Resolved, SCHEDULING, TAXONOMY, THEME, UPLOADS, USERS,
};

const FACADE: &str = include_str!("post-types.php");
const READING: &str = include_str!("posts.php");
const WRITING: &str = include_str!("post-writes.php");

thread_local! {
    /// The Hook API the native functions use, for the life of the worker.
    pub(crate) static SHARED: RefCell<Option<Rc<dyn HookApi>>> = const { RefCell::new(None) };
}

/// The native area of WordPress's post registries and post reading.
pub struct PostsArea {
    api: Rc<PostsApi>,
    writes: Rc<PostWritesApi>,
}

impl PostsArea {
    /// The registries, filtering and firing through `hooks`; the read use cases over `wpdb`'s `default` engine, the
    /// request's object cache, the metadata API and the options API; and the write use cases over the same, with
    /// `scheduling` (the Rust adapter over the cron scheduling core, or [`PhpScheduling`]) as the cron port's default
    /// binding. Their handlers are registered in `container`, the SQL table is bound as the default post repository,
    /// and the PHP adapters as the default bindings of the other container ports.
    pub fn new(
        container: Rc<Container>,
        hooks: Rc<dyn HookApi>,
        cache: Rc<dyn ObjectCache>,
        wpdb: Rc<dyn Wpdb>,
        metadata: Rc<dyn Metadata>,
        options: Rc<dyn Options>,
        scheduling: Rc<dyn Scheduling>,
    ) -> PostsArea {
        SHARED.with(|s| *s.borrow_mut() = Some(hooks.clone()));
        let sql: Rc<dyn PostRepository> = Rc::new(SqlPostRepository::new(wpdb, "default"));
        let default = sql.clone();
        container.bind_port::<dyn PostRepository>(POST_REPOSITORY, move |_, _| Ok(default.clone()));
        let terms: Rc<Resolved<dyn c::Taxonomy>> =
            Rc::new(Resolved::bind(&container, TAXONOMY, Rc::new(PhpTaxonomy)));
        let users: Rc<Resolved<dyn c::Users>> =
            Rc::new(Resolved::bind(&container, USERS, Rc::new(PhpUsers)));
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
            users: users.clone(),
            terms: terms.clone(),
            clock: Rc::new(PhpMicrotime),
            invalidation: Rc::new(PhpCacheInvalidation),
            options,
        });
        register_posts_handlers(&container, api.clone());
        let writes = PostWritesApi::new(WriteDeps {
            posts: api.clone(),
            supports: Rc::new(PhpPostTypeSupports),
            clock: Rc::new(PhpClock),
            users,
            taxonomy: terms,
            comments: Rc::new(Resolved::<dyn Comments>::bind(
                &container,
                COMMENTS,
                Rc::new(PhpComments),
            )),
            rewrite: Rc::new(Resolved::<dyn Rewrite>::bind(
                &container,
                REWRITE,
                Rc::new(PhpRewrite),
            )),
            uploads: Rc::new(Resolved::<dyn Uploads>::bind(
                &container,
                UPLOADS,
                Rc::new(PhpUploads),
            )),
            theme: Rc::new(Resolved::<dyn Theme>::bind(
                &container,
                THEME,
                Rc::new(PhpTheme),
            )),
            charset: Rc::new(Resolved::<dyn Charset>::bind(
                &container,
                CHARSET,
                Rc::new(PhpCharset),
            )),
            query: Rc::new(Resolved::<dyn PostQuery>::bind(
                &container,
                POST_QUERY,
                Rc::new(PhpPostQuery),
            )),
            truncation: Rc::new(PhpSlugTruncation),
            translator: Rc::new(PhpTranslator),
            formatting: Rc::new(PhpWriteFormatting),
            cron: Rc::new(Resolved::<dyn Scheduling>::bind(
                &container, SCHEDULING, scheduling,
            )),
            objects: Rc::new(PhpPostObjects),
            slugs: Rc::new(PhpSlugSettings),
            runtime: Rc::new(PhpRuntime),
        });
        register_post_write_handlers(&container, writes.clone());
        let reading = Rc::new(read::Reading::new(&container, current));
        read::READING.with(|r| *r.borrow_mut() = Some(reading));
        let writing = Rc::new(write::Writing::new(&container));
        write::WRITING.with(|w| *w.borrow_mut() = Some(writing));
        PostsArea { api, writes }
    }

    /// The post read API for Rust callers.
    pub fn api(&self) -> Rc<PostsApi> {
        self.api.clone()
    }

    /// The post write API for Rust callers.
    pub fn writes(&self) -> Rc<PostWritesApi> {
        self.writes.clone()
    }

    /// `area` with the post repository and the other container ports registered, so a PHP object implementing a
    /// port's twin (`SpeedySpec\WpPosts\PostRepository`, `…\Taxonomy`, `…\Users`, `…\Comments`, `…\Rewrite`,
    /// `…\Uploads`, `…\PostQuery`, `…\Theme`, `…\Charset`, `…\Scheduling`) and bound at its key reaches the use
    /// cases.
    pub fn ports(area: ContainerArea) -> ContainerArea {
        area.port::<dyn PostRepository>(POST_REPOSITORY, twins::wrap)
            .port::<dyn c::Taxonomy>(TAXONOMY, port_twins::taxonomy)
            .port::<dyn c::Users>(USERS, port_twins::users)
            .port::<dyn Comments>(COMMENTS, port_twins::comments)
            .port::<dyn Rewrite>(REWRITE, port_twins::rewrite)
            .port::<dyn Uploads>(UPLOADS, port_twins::uploads)
            .port::<dyn PostQuery>(POST_QUERY, port_twins::post_query)
            .port::<dyn Theme>(THEME, port_twins::theme)
            .port::<dyn Charset>(CHARSET, port_twins::charset)
            .port::<dyn Scheduling>(SCHEDULING, port_twins::scheduling)
    }
}

impl NativeArea for PostsArea {
    fn functions(&self) -> Vec<FunctionBuilder<'static>> {
        registry::functions()
    }

    fn request_start(&self) {
        codec::clear_live();
        codec::forget_renderings();
        for (label, source) in [
            ("speedyspec://wp-post-types", FACADE),
            ("speedyspec://wp-posts", READING),
            ("speedyspec://wp-post-writes", WRITING),
        ] {
            if let Err(err) = speedyspec_php::eval(label, source) {
                eprintln!("speedyspec_wordpress: installing the post functions failed: {err}");
            }
        }
    }

    fn request_end(&self) {
        codec::clear_live();
        codec::clear_renderings();
        let _ = adapters::take_parked();
        registry::reset_depth();
    }
}
