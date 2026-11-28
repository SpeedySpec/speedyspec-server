//! `PostWritesApi`: the post write use cases over their ports, and the helpers they share.
//!
//! Patterns: Command/CQRS (one handler per function, [`register_post_write_handlers`](crate::register_post_write_handlers)),
//! Template Method (`wp_insert_post()` as ordered steps, [`crate::write::insert`]), Observer (every filter and action
//! through [`HookApi`], the default callbacks registered as WordPress registers them), Repository
//! ([`PostRepository`], one statement per method), Data Mapper (`$data` to the [`Post`] entity the repository
//! writes), Strategy (hierarchical or flat slugs, through [`unique_post_slug_filtered`](crate::unique_post_slug_filtered)),
//! and Adapter for every port to unported code. Unit of Work is not used: `wp_insert_post()` keeps WordPress's
//! write-as-you-go order, so each statement is sent where WordPress sends it.

use std::rc::Rc;

use speedyspec_php_serialize::PhpValue;
use speedyspec_wp_db_contracts::{WpDbError, take_hook_error};
use speedyspec_wp_hooks_contracts::{HookApi, HookValue};
use speedyspec_wp_meta_contracts::{MetaKey, MetaType, ObjectId};
use speedyspec_wp_posts_contracts::{
    Charset, Clock, Comments, Post, PostError, PostId, PostQuery, PostReader, PostRef,
    PostRepository, PostTypeSupports, PostWriteError, Rewrite, SlugTruncation, Taxonomy, Theme,
    Translator, Uploads, Users, WriteResult,
};

use crate::PostsApi;
use crate::hook::pv;
use crate::read::{meta_error, options_error};
use crate::write::ports::{PostObjects, Runtime, Scheduling, SlugSettings, WriteFormatting};

/// What the post write use cases work through, beyond the read use cases'. The runtime passes the concrete adapters
/// in.
#[derive(Clone)]
pub struct WriteDeps {
    /// The read use cases: `get_post()`, `get_post_field()`, `clean_post_cache()` and the rest, with their hooks,
    /// cache, repository, metadata, options, registries and global `$post`.
    pub posts: Rc<PostsApi>,
    /// `post_type_supports()`.
    pub supports: Rc<dyn PostTypeSupports>,
    /// `current_time()`, `get_gmt_from_date()` and `strtotime()`.
    pub clock: Rc<dyn Clock>,
    /// `get_current_user_id()` and `current_user_can()`.
    pub users: Rc<dyn Users>,
    /// The term functions `wp_insert_post()` and `wp_update_post()` call.
    pub taxonomy: Rc<dyn Taxonomy>,
    /// `get_default_comment_status()`.
    pub comments: Rc<dyn Comments>,
    /// `get_permalink()`, for the GUID.
    pub rewrite: Rc<dyn Rewrite>,
    /// The attachment code `wp_insert_post()` and `wp_update_post()` hand over to.
    pub uploads: Rc<dyn Uploads>,
    /// `current_theme_supports()` and the page templates.
    pub theme: Rc<dyn Theme>,
    /// `$wpdb->get_col_charset()`, for the emoji check.
    pub charset: Rc<dyn Charset>,
    /// `get_posts()`, for the trashed posts holding a wanted slug.
    pub query: Rc<dyn PostQuery>,
    /// `wp_truncate_slug()`.
    pub truncation: Rc<dyn SlugTruncation>,
    /// `__()`, for `_doing_it_wrong()`'s message.
    pub translator: Rc<dyn Translator>,
    /// `sanitize_title()` and `wp_encode_emoji()`.
    pub formatting: Rc<dyn WriteFormatting>,
    /// `wp_clear_scheduled_hook()`, `wp_schedule_single_event()` and `wp_next_scheduled()`.
    pub cron: Rc<dyn Scheduling>,
    /// The `WP_Post` objects the hooks receive.
    pub objects: Rc<dyn PostObjects>,
    /// `$wp_rewrite`'s feeds and pagination base and the permalink structure, for `wp_unique_post_slug()`.
    pub slugs: Rc<dyn SlugSettings>,
    /// `defined()` and `_doing_it_wrong()`.
    pub runtime: Rc<dyn Runtime>,
}

/// WordPress's post writing (`wp_insert_post()`, `wp_update_post()`, `wp_transition_post_status()` and its default
/// callbacks, `set_post_type()`), one method per function. It handles every post write message
/// ([`register_post_write_handlers`](crate::register_post_write_handlers)).
///
/// `wp_check_post_hierarchy_for_loops()` breaks a loop by calling `wp_update_post()`, a real re-entry: the callback
/// holds a weak handle to this API ([`register_post_write_callbacks`](crate::register_post_write_callbacks)), so the
/// API and the hooks it registers on do not keep each other alive.
pub struct PostWritesApi {
    pub(crate) d: WriteDeps,
}

impl PostWritesApi {
    pub fn new(deps: WriteDeps) -> Rc<PostWritesApi> {
        Rc::new(PostWritesApi { d: deps })
    }

    pub fn deps(&self) -> &WriteDeps {
        &self.d
    }

    pub(crate) fn hooks(&self) -> &dyn HookApi {
        &*self.d.posts.deps().hooks
    }

    pub(crate) fn repository(&self) -> &dyn PostRepository {
        &*self.d.posts.deps().repository
    }

    /// `apply_filters()` on a PHP value, through the fast path when nothing listens.
    pub(crate) fn filter(
        &self,
        hook: &str,
        value: PhpValue,
        args: impl FnOnce() -> Vec<HookValue>,
    ) -> Result<PhpValue, PostError> {
        self.d.posts.filter(hook, value, args)
    }

    /// `do_action()`, through the fast path when nothing listens.
    pub(crate) fn action(
        &self,
        hook: &str,
        args: impl FnOnce() -> Vec<HookValue>,
    ) -> Result<(), PostError> {
        self.d.posts.action(hook, args)
    }

    /// `get_option( $name )` as a PHP value.
    pub(crate) fn option(&self, name: &str) -> Result<PhpValue, PostError> {
        let value = self
            .d
            .posts
            .deps()
            .options
            .get_option(name, None)
            .map_err(options_error)?;
        pv(&value)
    }

    /// `get_post_meta( $id, $key, $single )`.
    pub(crate) fn get_meta(
        &self,
        id: PostId,
        key: &[u8],
        single: bool,
    ) -> Result<PhpValue, PostError> {
        self.d
            .posts
            .deps()
            .metadata
            .get(
                &post_meta(),
                ObjectId(id.0),
                Some(&MetaKey::new(key.to_vec())),
                single,
            )
            .map_err(meta_error)
    }

    /// `add_post_meta( $id, $key, $value, $unique )`.
    pub(crate) fn add_meta(
        &self,
        id: PostId,
        key: &[u8],
        value: PhpValue,
        unique: bool,
    ) -> Result<(), PostError> {
        self.d
            .posts
            .deps()
            .metadata
            .add(
                &post_meta(),
                ObjectId(id.0),
                &MetaKey::new(key.to_vec()),
                value,
                unique,
            )
            .map_err(meta_error)?;
        Ok(())
    }

    /// `update_post_meta( $id, $key, $value )`.
    pub(crate) fn update_meta(
        &self,
        id: PostId,
        key: &[u8],
        value: PhpValue,
    ) -> Result<(), PostError> {
        self.d
            .posts
            .deps()
            .metadata
            .update(
                &post_meta(),
                ObjectId(id.0),
                &MetaKey::new(key.to_vec()),
                value,
                None,
            )
            .map_err(meta_error)?;
        Ok(())
    }

    /// `delete_post_meta( $id, $key )`.
    pub(crate) fn delete_meta(&self, id: PostId, key: &[u8]) -> Result<(), PostError> {
        self.d
            .posts
            .deps()
            .metadata
            .delete(
                &post_meta(),
                ObjectId(id.0),
                &MetaKey::new(key.to_vec()),
                None,
                false,
            )
            .map_err(meta_error)?;
        Ok(())
    }

    /// `get_post_field( $field, $id )`, in its default `display` context (so the field's display filter runs).
    pub(crate) fn post_field(&self, field: &str, id: PostId) -> Result<PhpValue, PostError> {
        self.d
            .posts
            .get_post_field(field.as_bytes(), PostRef::Id(id), "display")
    }

    /// `clean_post_cache( $id )`.
    pub(crate) fn clean_post_cache(&self, id: PostId) -> Result<(), PostError> {
        self.d.posts.clean_post_cache(PostRef::Id(id))
    }

    /// The `WP_Error` WordPress returns with `$wp_error`, or the `0` it returns without.
    pub(crate) fn refuse(wp_error: bool, error: PostWriteError) -> WriteResult {
        if wp_error { Err(error) } else { Ok(PostId(0)) }
    }
}

/// The `post` meta type.
pub(crate) fn post_meta() -> MetaType {
    MetaType::new("post").expect("post is a meta type")
}

/// Runs one repository write, then stops the use case when a `query` callback's exception was raised during it and
/// `$wpdb` hid it behind its answer (`take_hook_error()`), as WordPress would have thrown there.
///
/// Only a failure raised during this write counts: one already pending before it (raised and surfaced earlier) is
/// put back untouched, so a write that changed nothing is never failed by a stale error. A failure stays in `wpdb`'s
/// side channel for the PHP bridge to rethrow.
pub(crate) fn written<T>(write: impl FnOnce() -> Result<T, PostError>) -> Result<T, PostError> {
    let before = take_hook_error();
    let result = write();
    let during = take_hook_error();
    // `WpDbError::from` puts a failure back in the side channel; the first one stays first.
    if let Some(before) = before {
        let _: WpDbError = before.into();
    }
    match during {
        Some(hook) => {
            let _: WpDbError = hook.clone().into();
            match result {
                Err(e) => Err(e),
                Ok(_) => Err(PostError::Hook(hook)),
            }
        }
        None => result,
    }
}

/// `get_post( $id )` for `wp_unique_post_slug()`'s flat branch, which reads the post being written: id 0 (a new post)
/// falls back to the global `$post`, as `get_post( 0 )` does, through [`PostsApi::get_post`] and the
/// [`CurrentPost`](speedyspec_wp_posts_contracts::CurrentPost) port.
pub(crate) struct SlugReader<'a>(pub &'a PostsApi);

impl PostReader for SlugReader<'_> {
    fn post(&self, id: PostId) -> Result<Option<Post>, PostError> {
        self.0.get_post(PostRef::Id(id))
    }
}

/// A PHP int as a post id: 0 for anything below 1.
pub(crate) fn post_id(id: i64) -> PostId {
    PostId(u64::try_from(id).unwrap_or(0))
}

/// A post id as PHP's int.
pub(crate) fn int(id: PostId) -> i64 {
    i64::try_from(id.0).unwrap_or(i64::MAX)
}
