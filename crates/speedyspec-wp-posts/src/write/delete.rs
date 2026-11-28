//! `wp_delete_post( $post_id, $force_delete )` (post.php 4025-4213, WordPress 7.2-RC2) and the two default callbacks
//! default-filters.php hangs on deleting and trashing: `_reset_front_page_settings_for_post()` (4226-4245) and
//! `_reset_privacy_policy_page_for_post()` (4257-4261).
//!
//! The steps run in WordPress's order, each statement through its port: the row (`$wpdb->get_row()`, Repository),
//! the trash and attachment hand-offs, `pre_delete_post`, `before_delete_post`, the trash meta, the term
//! relationships (taxonomy port), the children of a hierarchical post, the revisions (revision port, which phase 6
//! replaces), the attachments, the comments (comment port), every meta row by id (meta's repository and
//! `delete_metadata_by_mid()`), the delete actions, the row itself, the caches and the `publish_future_post` event.
//! Hooks share one object, the `WP_Post` WordPress builds from the row it read (Observer).

use speedyspec_php_serialize::PhpValue;
use speedyspec_php_serialize::semantics::to_php_int;
use speedyspec_wp_hooks_contracts::HookValue;
use speedyspec_wp_posts_contracts::{
    PostArg, PostError, PostObject, PostRef, PostReturn, PostTypeName,
};

use crate::PostEntry;
use crate::is_post_type_hierarchical;
use crate::write::api::{PostWritesApi, int, post_id, written};
use crate::write::php::s;
use crate::write::transition::id_list;

impl PostWritesApi {
    /// `wp_delete_post( $post_id, $force_delete )`; `post_id` is `(int) $post_id`.
    ///
    /// - An id of 0 or below is `_doing_it_wrong()` and `false`; no row is `null`.
    /// - A post or page that is not trashed goes to [`PostWritesApi::wp_trash_post`] while `EMPTY_TRASH_DAYS` is set,
    ///   unless `force`; an attachment goes to `wp_delete_attachment()` (the uploads port). Their answers are
    ///   returned as they are.
    /// - Otherwise `pre_delete_post` may answer instead; then `before_delete_post`, the trash meta, the terms, the
    ///   children of a hierarchical post (pointed at the post's parent), each revision through
    ///   `wp_delete_post_revision()`, the attachments, the comments, every meta row, `delete_post_{$post_type}` and
    ///   `delete_post`, the row (`false` when nothing was deleted), `deleted_post_{$post_type}` and `deleted_post`,
    ///   `clean_post_cache()` for the post and each child, the `publish_future_post` event and `after_delete_post`.
    ///
    /// The answer is the post as it was read, with the handle its hooks received.
    pub fn wp_delete_post(&self, id: i64, force: bool) -> Result<PostReturn, PostError> {
        if id <= 0 {
            let message = self
                .d
                .translator
                .translate("The post ID must be greater than 0.", None)?;
            self.d
                .runtime
                .doing_it_wrong("wp_delete_post", &message, "6.9.0")?;
            return Ok(PostReturn::Value(PhpValue::Bool(false)));
        }
        let pid = post_id(id);
        let Some(row) = self.repository().raw_row(pid)? else {
            return Ok(PostReturn::Value(PhpValue::Null));
        };
        // `get_post( $post )` of the row `$wpdb` read: sanitized `raw` (which changes nothing) into a new `WP_Post`.
        let object = PostObject::new(row);
        let post_type = object.post.post_type.clone();
        if !force
            && matches!(post_type.as_str(), "post" | "page")
            && !crate::write::php::is_str(&self.d.posts.get_post_status(PostRef::Id(pid))?, "trash")
            && self.trash_enabled()?
        {
            return self.wp_trash_post(PostArg::Ref(PostRef::Id(pid)));
        }
        if "attachment" == post_type.as_str() {
            return Ok(PostReturn::Value(
                self.d.uploads.delete_attachment(pid, force)?,
            ));
        }

        let handle = self.d.objects.handle(&object)?;
        let check = self.filter("pre_delete_post", PhpValue::Null, || {
            vec![handle.clone(), HookValue::Bool(force)]
        })?;
        if !matches!(check, PhpValue::Null) {
            return Ok(PostReturn::Value(check));
        }
        let id_and_post = || vec![HookValue::Int(id), handle.clone()];
        self.action("before_delete_post", id_and_post)?;

        self.delete_meta(pid, b"_wp_trash_meta_status")?;
        self.delete_meta(pid, b"_wp_trash_meta_time")?;

        // `$post->post_type` and `$post->post_parent` as the object holds them now: a callback may have changed them.
        let now = self.current_post(&handle, &object)?;
        let post_type = now.post_type.clone();
        let taxonomies = self.d.taxonomy.object_taxonomies(&post_type)?;
        self.d
            .taxonomy
            .delete_object_term_relationships(pid, &taxonomies)?;

        let parent = now.parent;
        let types = &*self.d.posts.deps().types;
        let mut children = Vec::new();
        if is_post_type_hierarchical(types, post_type.as_str()) {
            children = self.repository().children_of_type(pid, &post_type)?;
            if !children.is_empty() {
                written(|| self.repository().reparent_children(pid, parent, &post_type))?;
            }
        }

        for revision in self.repository().revision_ids(pid)? {
            self.d.revisions.delete_post_revision(revision)?;
        }

        written(|| self.repository().reparent_attachments(pid, parent))?;

        self.d.comments.defer_counting(true)?;
        self.d.comments.delete_post_comments(pid)?;
        self.d.comments.defer_counting(false)?;

        for mid in self.post_meta_ids(pid)? {
            self.delete_meta_by_mid(mid)?;
        }

        let current_type = self.current_post(&handle, &object)?.post_type;
        self.action(&format!("delete_post_{current_type}"), id_and_post)?;
        self.action("delete_post", id_and_post)?;

        let deleted = written(|| self.repository().remove(pid))?;
        if !deleted.is_some_and(|rows| rows > 0) {
            return Ok(PostReturn::Value(PhpValue::Bool(false)));
        }

        let current_type = self.current_post(&handle, &object)?.post_type;
        self.action(&format!("deleted_post_{current_type}"), id_and_post)?;
        self.action("deleted_post", id_and_post)?;

        // `clean_post_cache( $post )` with the object itself: the row is gone, so it is not read again.
        self.clean_post_cache_of(&handle, &object)?;
        let current_type = self.current_post(&handle, &object)?.post_type;
        if is_post_type_hierarchical(types, current_type.as_str()) {
            for child in children {
                self.d.posts.clean_post_cache_object(PostEntry::wp_post(
                    PostEntry::raw_properties(&child),
                ))?;
            }
        }

        self.d
            .cron
            .clear_scheduled_hook("publish_future_post", &id_list(pid))
            .map_err(crate::write::ports::cron_error)?;

        self.action("after_delete_post", id_and_post)?;

        Ok(PostReturn::Post(Box::new(PostObject::with_handle(
            object.post,
            handle,
        ))))
    }

    /// The post's columns as the object `handle` holds them now; `object`'s own for a handle the port cannot read.
    pub(crate) fn current_post(
        &self,
        handle: &HookValue,
        object: &PostObject,
    ) -> Result<speedyspec_wp_posts_contracts::Post, PostError> {
        Ok(match self.d.objects.read(handle)? {
            Some(properties) => PostEntry::from_properties(&properties),
            None => object.post.clone(),
        })
    }

    /// `clean_post_cache( $post )` with the object a hook may have changed, as PHP passes it.
    pub(crate) fn clean_post_cache_of(
        &self,
        handle: &HookValue,
        object: &PostObject,
    ) -> Result<(), PostError> {
        let value = match self.d.objects.read(handle)? {
            Some(properties) => PostEntry::wp_post(properties),
            None => PostEntry::wp_post(PostEntry::raw_properties(&object.post)),
        };
        self.d.posts.clean_post_cache_object(value)
    }

    /// `_reset_front_page_settings_for_post( $post_id )`, on `before_delete_post` and `wp_trash_post`: for a page that
    /// is the front page, `show_on_front` becomes `posts` and `page_on_front` 0; for the posts page, `page_for_posts`
    /// 0; then the post is unstuck.
    ///
    /// With no post WordPress reads `$post->post_type` and `$post->ID` of null (warnings), so it unsticks id 0.
    pub fn reset_front_page_settings_for_post(&self, post: &PhpValue) -> Result<(), PostError> {
        let loaded = match post {
            PhpValue::Int(id) => self.d.posts.resolve(PostRef::Id(post_id(*id)))?,
            other => self.d.posts.resolve_value(other.clone())?,
        };
        let Some(loaded) = loaded else {
            return self.unstick_post(0);
        };
        let id = int(loaded.post.id);
        if "page" == loaded.post.post_type.as_str() {
            if to_php_int(&self.option("page_on_front")?) == id {
                self.update_option("show_on_front", s("posts"))?;
                self.update_option("page_on_front", PhpValue::Int(0))?;
            }
            if to_php_int(&self.option("page_for_posts")?) == id {
                self.update_option("page_for_posts", PhpValue::Int(0))?;
            }
        }
        self.unstick_post(id)
    }

    /// `_reset_privacy_policy_page_for_post( int $post_id )`, on `before_delete_post`: a page that is the privacy
    /// policy page sets `wp_page_for_privacy_policy` to 0.
    pub fn reset_privacy_policy_page_for_post(&self, id: i64) -> Result<(), PostError> {
        let post_type = match id {
            0 => self.d.posts.get_post_type(PostRef::Current)?,
            id if id > 0 => self.d.posts.get_post_type(PostRef::Id(post_id(id)))?,
            _ => None,
        };
        if post_type.as_ref().map(PostTypeName::as_str) == Some("page")
            && to_php_int(&self.option("wp_page_for_privacy_policy")?) == id
        {
            self.update_option("wp_page_for_privacy_policy", PhpValue::Int(0))?;
        }
        Ok(())
    }
}
