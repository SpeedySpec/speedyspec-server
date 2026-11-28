//! `wp_trash_post( $post_id )` (post.php 4275-4348) and `wp_untrash_post( $post_id )` (4359-4449), WordPress
//! 7.2-RC2.
//!
//! Both write through `wp_update_post()` (a real re-entry), which handles the `__trashed` slug suffix and the
//! `_wp_desired_post_slug` round trip ([`crate::write::insert`]); the comments go through the comment port. Hooks
//! receive the object `get_post()` gave, or the caller's own (Observer).

use speedyspec_php_serialize::PhpValue;
use speedyspec_wp_hooks_contracts::HookValue;
use speedyspec_wp_posts_contracts::{PostArg, PostError, PostId, PostRef, PostReturn};

use crate::hook::hv;
use crate::write::api::{PostWritesApi, handled, int};
use crate::write::php::{key, s};

/// What `wp_update_post()` answered, by PHP truth: a `WP_Error` object is truthy, `0` is not.
fn updated(answer: &speedyspec_wp_posts_contracts::WriteResult) -> bool {
    !matches!(answer, Ok(PostId(0)))
}

impl PostWritesApi {
    /// `wp_trash_post( $post_id )`:
    ///
    /// - without `EMPTY_TRASH_DAYS`, [`PostWritesApi::wp_delete_post`] with `force`;
    /// - no post is `null`, a trashed one `false`;
    /// - `pre_trash_post` may answer instead;
    /// - then `wp_trash_post`, the trash meta (`_wp_trash_meta_status` and `_wp_trash_meta_time`),
    ///   `wp_update_post( array( 'ID' => $post_id, 'post_status' => 'trash' ) )` (`false` when it answers 0), the
    ///   comments, and `trashed_post`.
    ///
    /// The answer is the post as it was read: the caller's object when it passed one.
    ///
    /// `$post_id` is the argument as given: 0 for the global `$post` (so `add_post_meta( 0, … )` writes nothing and
    /// `wp_update_post()` reads the global post, as WordPress's does). A known divergence for an object argument:
    /// WordPress passes the object itself where it wants an id (the hooks get it as it is, `add_post_meta()` casts
    /// it to 1 with a warning); here the hooks receive the object and everything else its `ID`.
    pub fn wp_trash_post(&self, post: PostArg) -> Result<PostReturn, PostError> {
        let (arg_id, hook_id) = match &post {
            PostArg::Ref(PostRef::Id(id)) => (*id, None),
            PostArg::Ref(PostRef::Current) => (PostId(0), None),
            PostArg::Object(object) => (object.post.id, Some(object.clone())),
        };
        if !self.trash_enabled()? {
            return self.wp_delete_post(int(arg_id), true);
        }
        let Some(object) = self.post_arg(post)? else {
            return Ok(PostReturn::Value(PhpValue::Null));
        };
        if "trash" == object.post.status.as_str() {
            return Ok(PostReturn::Value(PhpValue::Bool(false)));
        }
        let previous = object.post.status.as_str().to_owned();
        let handle = self.d.objects.handle(&object)?;
        let id_arg = match hook_id {
            Some(_) => handle.clone(),
            None => HookValue::Int(int(arg_id)),
        };
        let check = self.filter("pre_trash_post", PhpValue::Null, || {
            vec![handle.clone(), HookValue::Str(previous.clone())]
        })?;
        if !matches!(check, PhpValue::Null) {
            return Ok(PostReturn::Value(check));
        }
        self.action("wp_trash_post", || {
            vec![id_arg.clone(), HookValue::Str(previous.clone())]
        })?;

        self.add_meta(arg_id, b"_wp_trash_meta_status", s(&previous), false)?;
        self.add_meta(
            arg_id,
            b"_wp_trash_meta_time",
            PhpValue::Int(self.time()),
            false,
        )?;

        let answer = self.wp_update_post(
            PhpValue::Array(vec![
                (key("ID"), PhpValue::Int(int(arg_id))),
                (key("post_status"), s("trash")),
            ]),
            false,
            true,
        )?;
        if !updated(&answer) {
            return Ok(PostReturn::Value(PhpValue::Bool(false)));
        }

        self.d.comments.trash_post_comments(arg_id)?;

        self.action("trashed_post", || {
            vec![id_arg.clone(), HookValue::Str(previous.clone())]
        })?;

        Ok(PostReturn::Post(Box::new(handled(object, handle))))
    }

    /// `wp_untrash_post( $post_id )`:
    ///
    /// - no post is `null`, one not in the trash `false`;
    /// - `pre_untrash_post` may answer instead, with `_wp_trash_meta_status` as the previous status;
    /// - then `untrash_post`, the status (`draft`, or `inherit` for an attachment) through `wp_untrash_post_status`,
    ///   the trash meta deleted, `wp_update_post()` with that status (`false` when it answers 0), the comments, and
    ///   `untrashed_post`.
    ///
    /// The answer is the post as it was read: the caller's object when it passed one.
    pub fn wp_untrash_post(&self, post: PostArg) -> Result<PostReturn, PostError> {
        let Some(object) = self.post_arg(post)? else {
            return Ok(PostReturn::Value(PhpValue::Null));
        };
        let id = object.post.id;
        if "trash" != object.post.status.as_str() {
            return Ok(PostReturn::Value(PhpValue::Bool(false)));
        }
        let previous = self.get_meta(id, b"_wp_trash_meta_status", true)?;
        let handle = self.d.objects.handle(&object)?;
        let check = self.filter("pre_untrash_post", PhpValue::Null, || {
            vec![handle.clone(), hv(&previous)]
        })?;
        if !matches!(check, PhpValue::Null) {
            return Ok(PostReturn::Value(check));
        }
        let id_hook = || HookValue::Int(int(id));
        self.action("untrash_post", || vec![id_hook(), hv(&previous)])?;

        let new_status = if "attachment" == object.post.post_type.as_str() {
            "inherit"
        } else {
            "draft"
        };
        let status = self.filter("wp_untrash_post_status", s(new_status), || {
            vec![id_hook(), hv(&previous)]
        })?;

        self.delete_meta(id, b"_wp_trash_meta_status")?;
        self.delete_meta(id, b"_wp_trash_meta_time")?;

        let answer = self.wp_update_post(
            PhpValue::Array(vec![
                (key("ID"), PhpValue::Int(int(id))),
                (key("post_status"), status),
            ]),
            false,
            true,
        )?;
        if !updated(&answer) {
            return Ok(PostReturn::Value(PhpValue::Bool(false)));
        }

        self.d.comments.untrash_post_comments(id)?;

        self.action("untrashed_post", || vec![id_hook(), hv(&previous)])?;

        Ok(PostReturn::Post(Box::new(handled(object, handle))))
    }
}
