//! `wp_publish_post( $post )` (post.php 5645-5713) and `check_and_publish_future_post( $post )` (5723-5745),
//! WordPress 7.2-RC2.
//!
//! `wp_publish_post()` gives each taxonomy with a default term one if the post has none (taxonomy port), writes the
//! status (Repository), then fires the transition and the `save_post` family with one object, the caller's when it
//! passed one (Observer). `check_and_publish_future_post()` reschedules through the cron contracts (Adapter) a post
//! whose time has not come.

use speedyspec_php_serialize::semantics::{is_empty, to_php_int};
use speedyspec_php_serialize::{PhpKey, PhpValue};
use speedyspec_wp_hooks_contracts::HookValue;
use speedyspec_wp_posts_contracts::{PostArg, PostError, PostObject, PostRef, PostStatusName};

use crate::write::api::{PostWritesApi, int, written};
use crate::write::php::s;
use crate::write::ports::cron_error;
use crate::write::transition::id_list;

impl PostWritesApi {
    /// `wp_publish_post( $post )`: nothing for no post or a published one. Otherwise `$post_before = get_post(
    /// $post->ID )`; for `category` and each taxonomy with a default term, a post with no terms there gets
    /// `default_category` (or `default_term_{$taxonomy}`) when it is set; then ``UPDATE … SET `post_status` =
    /// 'publish'``, `clean_post_cache()`, `$post->post_status = 'publish'` and `wp_transition_post_status()`, then
    /// `edit_post_{$post_type}`, `edit_post`, `save_post_{$post_type}`, `save_post`, `wp_insert_post` and
    /// `wp_after_insert_post`, all with the one object.
    pub fn wp_publish_post(&self, post: PostArg) -> Result<(), PostError> {
        let Some(object) = self.post_arg(post)? else {
            return Ok(());
        };
        if "publish" == object.post.status.as_str() {
            return Ok(());
        }
        let id = object.post.id;
        let before = match self.d.posts.resolve(PostRef::Id(id))? {
            Some(loaded) => self.d.objects.handle(&PostObject::new(loaded.post))?,
            None => HookValue::Null,
        };

        let post_type = &object.post.post_type;
        let with_default = self.d.taxonomy.default_terms(post_type)?;
        for taxonomy in self.d.taxonomy.object_taxonomies(post_type)? {
            if "category" != taxonomy && !with_default.contains(&taxonomy) {
                continue;
            }
            if !is_empty(&self.d.taxonomy.get_the_terms(id, &taxonomy)?) {
                continue;
            }
            let option = if "category" == taxonomy {
                "default_category".to_owned()
            } else {
                format!("default_term_{taxonomy}")
            };
            let term = to_php_int(&self.option_or(&option, PhpValue::Int(0))?);
            if term == 0 {
                continue;
            }
            let terms = PhpValue::Array(vec![(PhpKey::Int(0), PhpValue::Int(term))]);
            self.d
                .taxonomy
                .set_post_terms(id, &terms, &taxonomy, false)?;
        }

        let publish = PostStatusName::new("publish");
        written(|| self.repository().update_status(id, &publish))?;
        self.clean_post_cache(id)?;

        let old = object.post.status.clone();
        // `$post->post_status = 'publish'` on the object the hooks share.
        let object = match &object.handle {
            Some(handle) => {
                let handle = self.d.objects.set(handle, "post_status", s("publish"))?;
                let mut changed = object.clone();
                changed.handle = Some(handle);
                changed.post.status = publish.clone();
                changed
            }
            None => {
                let mut changed = object.clone();
                changed.post.status = publish.clone();
                changed
            }
        };
        let object = self.wp_transition_post_status(&publish, &old, &object)?;
        let handle = self.d.objects.handle(&object)?;

        // `$post->ID` and `$post->post_type` are read at each hook, as PHP reads the object then.
        let (post_id, post_type) = self.id_and_type_of(&handle, &object)?;
        self.action(&format!("edit_post_{post_type}"), || {
            vec![HookValue::Int(int(post_id)), handle.clone()]
        })?;
        let (post_id, _) = self.id_and_type_of(&handle, &object)?;
        self.action("edit_post", || {
            vec![HookValue::Int(int(post_id)), handle.clone()]
        })?;
        let saved = |post_id| {
            vec![
                HookValue::Int(int(post_id)),
                handle.clone(),
                HookValue::Bool(true),
            ]
        };
        let (post_id, post_type) = self.id_and_type_of(&handle, &object)?;
        self.action(&format!("save_post_{post_type}"), || saved(post_id))?;
        let (post_id, _) = self.id_and_type_of(&handle, &object)?;
        self.action("save_post", || saved(post_id))?;
        let (post_id, _) = self.id_and_type_of(&handle, &object)?;
        self.action("wp_insert_post", || saved(post_id))?;
        self.after_insert_post(&handle, post_id, true, before)
    }

    /// `check_and_publish_future_post( $post )`: nothing for no post or one that is not `future`. A post whose
    /// `strtotime( $post->post_date_gmt . ' GMT' )` is still ahead of `time()` has `publish_future_post` cleared and
    /// scheduled again at that time; any other is published by id ([`PostWritesApi::wp_publish_post`]).
    ///
    /// A date `strtotime()` cannot read is `false`, which PHP's `false > time()` treats as not ahead, so the post is
    /// published.
    pub fn check_and_publish_future_post(&self, post: PostArg) -> Result<(), PostError> {
        let Some(object) = self.post_arg(post)? else {
            return Ok(());
        };
        if "future" != object.post.status.as_str() {
            return Ok(());
        }
        let id = object.post.id;
        let mut gmt = object.post.date.gmt.as_str().as_bytes().to_vec();
        gmt.extend_from_slice(b" GMT");
        if let Some(time) = self.d.clock.timestamp(&gmt)
            && time > self.time()
        {
            let cron = &*self.d.cron;
            cron.clear_scheduled_hook("publish_future_post", &id_list(id))
                .map_err(cron_error)?;
            cron.schedule_single_event(&HookValue::Int(time), "publish_future_post", &id_list(id))
                .map_err(cron_error)?;
            return Ok(());
        }
        self.wp_publish_post(PostArg::Ref(PostRef::Id(id)))
    }
}
