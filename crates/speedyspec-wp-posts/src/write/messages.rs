//! Every post write function as a use case on the buses (Command / CQRS), handled by [`PostWritesApi`]. The keys
//! are `SpeedySpec\WpPosts\<Name>Handler`; PHP overrides one by binding the same key.
//!
//! The default callbacks have messages of their own (`TransitionPostStatusDefault`, `PublishPostHook`,
//! `FuturePostHook`, `CheckPostHierarchyForLoops`, `ResetFrontPageSettingsForPost`, `ResetPrivacyPolicyPageForPost`;
//! `check_and_publish_future_post()` is its own function's message), so the PHP functions WordPress registers by name dispatch to
//! the same handlers the Rust callbacks call.

use std::rc::Rc;

use speedyspec_container::Container;
use speedyspec_cqrs::{Command, CommandHandler, bind_command_handler};
use speedyspec_php_serialize::PhpValue;
use speedyspec_wp_posts_contracts::{
    PostArg, PostError, PostId, PostObject, PostReturn, PostStatusName, WriteResult,
};

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

command!(WpDeletePost { post_id: i64, force: bool } -> R<PostReturn>,
    |a, m| a.wp_delete_post(m.post_id, m.force));
command!(WpTrashPost { post: PostArg } -> R<PostReturn>, |a, m| a.wp_trash_post(m.post));
command!(WpUntrashPost { post: PostArg } -> R<PostReturn>, |a, m| a.wp_untrash_post(m.post));
command!(WpPublishPost { post: PostArg } -> R<()>, |a, m| a.wp_publish_post(m.post));
command!(CheckAndPublishFuturePost { post: PostArg } -> R<()>,
    |a, m| a.check_and_publish_future_post(m.post));
command!(StickPost { post_id: i64 } -> R<()>, |a, m| a.stick_post(m.post_id));
command!(UnstickPost { post_id: i64 } -> R<()>, |a, m| a.unstick_post(m.post_id));
command!(AddPing { post: PostArg, uri: PhpValue } -> R<Option<u64>>, |a, m| a.add_ping(m.post, &m.uri));
command!(WpDeleteAutoDrafts {} -> R<()>, |a, _m| a.wp_delete_auto_drafts());
command!(ResetFrontPageSettingsForPost { post_id: PhpValue } -> R<()>,
    |a, m| a.reset_front_page_settings_for_post(&m.post_id));
command!(ResetPrivacyPolicyPageForPost { post_id: i64 } -> R<()>,
    |a, m| a.reset_privacy_policy_page_for_post(m.post_id));

macro_rules! bind_all {
    ($container:expr, $api:expr; $($c:ty),*) => {
        $( { let a = $api.clone(); bind_command_handler::<$c>($container, move |_, _| Ok(a.clone())); } )*
    };
}

/// Binds `api` as the handler of every post write message, in `post.php` order; PHP overrides one by binding the
/// same key.
pub fn register_post_write_handlers(container: &Container, api: Rc<PostWritesApi>) {
    bind_all!(container, api; StickPost, UnstickPost, SetPostType, WpDeletePost,
        ResetFrontPageSettingsForPost, ResetPrivacyPolicyPageForPost, WpTrashPost, WpUntrashPost, WpInsertPost,
        WpUpdatePost, WpPublishPost, CheckAndPublishFuturePost, WpTransitionPostStatus, AddPing,
        TransitionPostStatusDefault, FuturePostHook, PublishPostHook, CheckPostHierarchyForLoops,
        WpDeleteAutoDrafts);
}
