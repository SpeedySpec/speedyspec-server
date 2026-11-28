//! Every post write function as a use case on the buses (Command / CQRS), handled by [`PostWritesApi`]. The keys
//! are `SpeedySpec\WpPosts\<Name>Handler`; PHP overrides one by binding the same key.
//!
//! The default callbacks have messages of their own (`TransitionPostStatusDefault`, `PublishPostHook`,
//! `FuturePostHook`, `CheckPostHierarchyForLoops`), so the PHP functions WordPress registers by name dispatch to
//! the same handlers the Rust callbacks call.

use std::rc::Rc;

use speedyspec_container::Container;
use speedyspec_cqrs::{Command, CommandHandler, bind_command_handler};
use speedyspec_php_serialize::PhpValue;
use speedyspec_wp_posts_contracts::{PostError, PostId, PostObject, PostStatusName, WriteResult};

use crate::PostWritesApi;

macro_rules! command {
    ($name:ident { $($field:ident : $ty:ty),* } -> $result:ty, |$a:ident, $m:ident| $body:expr) => {
        #[derive(Clone, Debug)]
        pub struct $name { $(pub $field: $ty),* }
        impl Command for $name {
            type Result = $result;
            const KEY: &'static str = concat!("SpeedySpec\\WpPosts\\", stringify!($name), "Handler");
        }
        impl CommandHandler<$name> for PostWritesApi {
            fn handle(&self, $m: $name) -> $result {
                let $a = self;
                $body
            }
        }
    };
}

type R<T> = Result<T, PostError>;

command!(WpInsertPost { postarr: PhpValue, wp_error: bool, fire_after_hooks: bool } -> R<WriteResult>,
    |a, m| a.wp_insert_post(m.postarr, m.wp_error, m.fire_after_hooks));
command!(WpUpdatePost { postarr: PhpValue, wp_error: bool, fire_after_hooks: bool } -> R<WriteResult>,
    |a, m| a.wp_update_post(m.postarr, m.wp_error, m.fire_after_hooks));
command!(WpTransitionPostStatus { new: PostStatusName, old: PostStatusName, post: PostObject } -> R<PostObject>,
    |a, m| a.wp_transition_post_status(&m.new, &m.old, &m.post));
command!(SetPostType { post_id: i64, post_type: PhpValue } -> R<Option<u64>>,
    |a, m| a.set_post_type(m.post_id, &m.post_type));
command!(TransitionPostStatusDefault { new: String, old: String, post_id: PostId, post_type: String } -> R<()>,
    |a, m| a.transition_post_status_default(&m.new, &m.old, m.post_id, &m.post_type));
command!(PublishPostHook { post_id: i64 } -> R<()>, |a, m| a.publish_post_hook(m.post_id));
command!(FuturePostHook { post_id: PostId, post_date: Vec<u8> } -> R<()>,
    |a, m| a.future_post_hook(m.post_id, &m.post_date));
command!(CheckPostHierarchyForLoops { post_parent: PhpValue, post_id: PhpValue } -> R<PhpValue>,
    |a, m| a.check_post_hierarchy_for_loops_filter(m.post_parent, m.post_id));

macro_rules! bind_all {
    ($container:expr, $api:expr; $($c:ty),*) => {
        $( { let a = $api.clone(); bind_command_handler::<$c>($container, move |_, _| Ok(a.clone())); } )*
    };
}

/// Binds `api` as the handler of every post write message, in `post.php` order; PHP overrides one by binding the
/// same key.
pub fn register_post_write_handlers(container: &Container, api: Rc<PostWritesApi>) {
    bind_all!(container, api; SetPostType, WpInsertPost, WpUpdatePost, WpTransitionPostStatus,
        TransitionPostStatusDefault, FuturePostHook, PublishPostHook, CheckPostHierarchyForLoops);
}
