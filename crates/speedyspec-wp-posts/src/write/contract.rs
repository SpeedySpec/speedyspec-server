//! [`PostWrites`], the posts write contract, over [`PostWritesApi`]: each method is the use case of the WordPress
//! function it names (Command/CQRS: the command side, apart from the [`Posts`](speedyspec_wp_posts_contracts::Posts)
//! queries).

use speedyspec_php_serialize::PhpValue;
use speedyspec_wp_posts_contracts::{
    PostArg, PostError, PostObject, PostReturn, PostStatusName, PostWrites, WriteResult,
};

use crate::write::api::PostWritesApi;

impl PostWrites for PostWritesApi {
    fn insert(
        &self,
        postarr: PhpValue,
        wp_error: bool,
        fire_after_hooks: bool,
    ) -> Result<WriteResult, PostError> {
        self.wp_insert_post(postarr, wp_error, fire_after_hooks)
    }

    fn update(
        &self,
        postarr: PhpValue,
        wp_error: bool,
        fire_after_hooks: bool,
    ) -> Result<WriteResult, PostError> {
        self.wp_update_post(postarr, wp_error, fire_after_hooks)
    }

    fn delete(&self, post_id: i64, force: bool) -> Result<PostReturn, PostError> {
        self.wp_delete_post(post_id, force)
    }

    fn trash(&self, post: PostArg) -> Result<PostReturn, PostError> {
        self.wp_trash_post(post)
    }

    fn untrash(&self, post: PostArg) -> Result<PostReturn, PostError> {
        self.wp_untrash_post(post)
    }

    fn publish(&self, post: PostArg) -> Result<(), PostError> {
        self.wp_publish_post(post)
    }

    fn check_and_publish_future(&self, post: PostArg) -> Result<(), PostError> {
        self.check_and_publish_future_post(post)
    }

    fn transition(
        &self,
        new: &PostStatusName,
        old: &PostStatusName,
        post: &PostObject,
    ) -> Result<PostObject, PostError> {
        self.wp_transition_post_status(new, old, post)
    }

    fn stick(&self, post_id: i64) -> Result<(), PostError> {
        self.stick_post(post_id)
    }

    fn unstick(&self, post_id: i64) -> Result<(), PostError> {
        self.unstick_post(post_id)
    }

    fn add_ping(&self, post: PostArg, uri: &PhpValue) -> Result<Option<u64>, PostError> {
        PostWritesApi::add_ping(self, post, uri)
    }

    fn set_post_type(&self, post_id: i64, post_type: &PhpValue) -> Result<Option<u64>, PostError> {
        PostWritesApi::set_post_type(self, post_id, post_type)
    }

    fn delete_auto_drafts(&self) -> Result<(), PostError> {
        self.wp_delete_auto_drafts()
    }
}
