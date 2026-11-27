//! WordPress's post type and post status registries in PHP, native over the Rust post type rules (Facade, Adapter).
//!
//! [`PostsArea`] declares, at the start of every request, the 20 registry functions of `post.php` this port covers
//! (`register_post_type()` and its readers, `register_post_status()` and its readers, the supports functions, the
//! label and capability helpers, and `create_initial_post_types()`) with WordPress's signatures, from
//! `post-types.php`. Each is one line calling a native function under `SpeedySpec\WpPosts\Native\`.
//!
//! The area depends on contracts only: the Hook API comes in as `Rc<dyn HookApi>` from the runtime. The registries
//! are PHP's globals, read live during a request (Registry: [`PhpPostTypes`], [`PhpPostStatuses`],
//! [`PhpPostTypeSupports`]); the other outbound ports are PHP adapters ([`PhpTranslator`], [`PhpRewriteContext`],
//! [`PhpTitleSanitizer`]). `WP_Post_Type` stays WordPress's class (see the `registry` module).
//!
//! The area has no container port yet: the registries are PHP's globals. The reading task adds the `PostRepository`
//! port with `PostsArea::ports`, chained in the runtime's `worker.rs` after `MetaArea::ports`.
//!
//! Patterns: Facade (the PHP functions), Adapter (the ports over PHP and its globals), Registry (`$wp_post_types`,
//! `$wp_post_statuses` and `$_wp_post_type_features`, read live), Observer (the hooks, through [`HookApi`]),
//! Command (`create_initial_post_types()`' table of calls, from `speedyspec-wp-posts`).

mod adapters;
mod codec;
mod registry;
mod release;

use std::cell::RefCell;
use std::rc::Rc;

use speedyspec_php::NativeArea;
use speedyspec_php::ext_php_rs::builders::FunctionBuilder;
use speedyspec_wp_hooks_contracts::HookApi;

pub use adapters::{
    PhpPostStatuses, PhpPostTypeSupports, PhpPostTypes, PhpRewriteContext, PhpTitleSanitizer,
    PhpTranslator,
};
pub use codec::{Thrown, from_php, to_php};
pub use release::{PORTED_FUNCTIONS, prepare_release, strip_ported};

const FACADE: &str = include_str!("post-types.php");

thread_local! {
    /// The Hook API the native functions use, for the life of the worker.
    pub(crate) static SHARED: RefCell<Option<Rc<dyn HookApi>>> = const { RefCell::new(None) };
}

/// The native area of WordPress's post type and post status registries.
pub struct PostsArea {
    _hooks: Rc<dyn HookApi>,
}

impl PostsArea {
    /// The registries, filtering and firing through `hooks`.
    pub fn new(hooks: Rc<dyn HookApi>) -> PostsArea {
        SHARED.with(|s| *s.borrow_mut() = Some(hooks.clone()));
        PostsArea { _hooks: hooks }
    }
}

impl NativeArea for PostsArea {
    fn functions(&self) -> Vec<FunctionBuilder<'static>> {
        registry::functions()
    }

    fn request_start(&self) {
        codec::clear_live();
        if let Err(err) = speedyspec_php::eval("speedyspec://wp-post-types", FACADE) {
            eprintln!("speedyspec_wordpress: installing the post type functions failed: {err}");
        }
    }

    fn request_end(&self) {
        codec::clear_live();
        let _ = adapters::take_parked();
        registry::reset_depth();
    }
}
