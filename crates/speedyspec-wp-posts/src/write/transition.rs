//! `wp_transition_post_status()` (post.php 6128-6177, WordPress 7.2-RC2) and the default callbacks WordPress
//! registers around writing posts, registered here as WordPress registers them (Observer):
//!
//! | Hook | Callback | Priority, arguments | Registered by |
//! |---|---|---|---|
//! | `wp_insert_post_parent` | `wp_check_post_hierarchy_for_loops()` (post.php 8527-8565) | 10, 2 | default-filters.php 193 |
//! | `publish_post` | `_publish_post_hook()` (post.php 8462-8492) | 5, 1 | default-filters.php 453 |
//! | `transition_post_status` | `_transition_post_status()` (post.php 8396-8431) | 5, 3 | default-filters.php 454 |
//! | `future_{$post_type}` | `_future_post_hook()` (post.php 8447-8450) | 5, 2 | `WP_Post_Type::add_hooks()` |
//!
//! Each callback's id is the WordPress function's name, so `remove_action( 'transition_post_status',
//! '_transition_post_status', 5 )` removes it. The callbacks read the post object a hook received through the
//! [`PostObjects`](crate::PostObjects) port, as PHP reads the object then.

use std::rc::{Rc, Weak};

use speedyspec_php_serialize::semantics::{is_empty, to_php_int, to_php_string};
use speedyspec_php_serialize::{PhpKey, PhpValue};
use speedyspec_wp_hooks_contracts::{Callback, HookApi, HookError, HookName, HookValue};
use speedyspec_wp_posts_contracts::{
    PostError, PostId, PostObject, PostRef, PostStatusName, PostTypeName,
};

use crate::check_post_hierarchy_for_loops;
use crate::hook::{hv, pv};
use crate::read::Reader;
use crate::write::api::{PostWritesApi, checked_rows, int, post_id};
use crate::write::insert::id_and_type;
use crate::write::php::{at, identical, is_str, key, s};
use crate::write::ports::cron_error;

/// `array( $post_id )`, the cron arguments of `publish_future_post`.
fn id_list(id: PostId) -> HookValue {
    HookValue::Array(vec![(
        speedyspec_wp_hooks_contracts::Key::Int(0),
        HookValue::Int(int(id)),
    )])
}

/// A hook argument as text.
fn text_arg(value: &HookValue) -> Result<String, PostError> {
    Ok(String::from_utf8_lossy(&to_php_string(&pv(value)?)).into_owned())
}

impl PostWritesApi {
    /// `wp_transition_post_status( $new_status, $old_status, $post )`: `transition_post_status`, then
    /// `{$old_status}_to_{$new_status}` when the new status is registered and the old one is too (or `new`), then
    /// `{$new_status}_{$post->post_type}` when the new status and the post type are registered. The three hooks
    /// receive one object, `post`'s handle or the one [`PostObjects::handle`](crate::PostObjects::handle) builds;
    /// the answer is `post` with that handle set, for the hooks the caller fires afterwards.
    ///
    /// `$post->post_type` is read once the first action has run, and `$post->ID` and `$post->post_type` again for
    /// the last hook, as PHP reads the object at those points.
    pub fn wp_transition_post_status(
        &self,
        new: &PostStatusName,
        old: &PostStatusName,
        post: &PostObject,
    ) -> Result<PostObject, PostError> {
        let handle = self.d.objects.handle(post)?;
        self.action("transition_post_status", || {
            vec![
                HookValue::Str(new.as_str().to_owned()),
                HookValue::Str(old.as_str().to_owned()),
                handle.clone(),
            ]
        })?;
        let deps = self.d.posts.deps();
        let new_known = deps.statuses.status(new.as_str()).is_some();
        let old_valid = deps.statuses.status(old.as_str()).is_some() || "new" == old.as_str();
        let (_, post_type) = self.read_post(&handle, post)?;
        let type_known = deps.types.post_type(&post_type).is_some();
        if new_known && old_valid {
            self.action(&format!("{old}_to_{new}"), || vec![handle.clone()])?;
        }
        if new_known && type_known {
            let (id, post_type) = self.read_post(&handle, post)?;
            self.action(&format!("{new}_{post_type}"), || {
                vec![
                    HookValue::Int(int(id)),
                    handle.clone(),
                    HookValue::Str(old.as_str().to_owned()),
                ]
            })?;
        }
        let mut answer = post.clone();
        answer.handle = Some(handle);
        Ok(answer)
    }

    /// `$post->ID` and `$post->post_type` as the object holds them now; `post`'s own values for a handle the port
    /// cannot read.
    fn read_post(
        &self,
        handle: &HookValue,
        post: &PostObject,
    ) -> Result<(PostId, String), PostError> {
        Ok(match self.d.objects.read(handle)? {
            Some(properties) => id_and_type(&properties),
            None => (post.post.id, post.post.post_type.as_str().to_owned()),
        })
    }

    /// `get_the_guid( $post_id )`: the post's `guid` (`''` for none) through the `get_the_guid` filter.
    fn the_guid(&self, id: PostId) -> Result<PhpValue, PostError> {
        let (guid, id) = match self.d.posts.instance(int(id))? {
            Some(loaded) => (s(&loaded.post.guid), loaded.post.id),
            None => (s(""), PostId(0)),
        };
        self.filter("get_the_guid", guid, || vec![HookValue::Int(int(id))])
    }

    /// `_transition_post_status( $new_status, $old_status, $post )` for the post `id` of `post_type`:
    /// - first published: a post with no GUID takes its permalink (`get_the_guid()`'s filter decides), then the
    ///   deprecated `private_to_published` action;
    /// - either status `publish`: the `lastpostmodified` and `lastpostdate` entries of `timeinfo` for every time zone;
    /// - a status change: the type's two `counts` entries;
    /// - always: `publish_future_post` unscheduled for the post.
    pub fn transition_post_status_default(
        &self,
        new: &str,
        old: &str,
        id: PostId,
        post_type: &str,
    ) -> Result<(), PostError> {
        let posts = &self.d.posts;
        if "publish" != old && "publish" == new {
            if is_str(&self.the_guid(id)?, "") {
                let guid = self.d.rewrite.permalink(id)?.unwrap_or_default();
                checked_rows(self.repository().update_guid(id, &guid)?)?;
            }
            self.hooks().do_action_deprecated(
                &HookName::from("private_to_published"),
                vec![HookValue::Int(int(id))],
                &HookValue::Str("2.3.0".into()),
                &HookValue::Str("private_to_publish".into()),
                &HookValue::Str(String::new()),
            )?;
        }
        if "publish" == new || "publish" == old {
            for timezone in ["server", "gmt", "blog"] {
                for key in [
                    format!("lastpostmodified:{timezone}"),
                    format!("lastpostdate:{timezone}"),
                    format!("lastpostdate:{timezone}:{post_type}"),
                ] {
                    posts.cache_delete(&HookValue::Str(key), "timeinfo")?;
                }
            }
        }
        if new != old {
            for perm in ["", "readable"] {
                let key = posts.count_posts_cache_key(post_type, perm)?;
                posts.cache_delete(&HookValue::Str(key), "counts")?;
            }
        }
        self.d
            .cron
            .clear_scheduled_hook("publish_future_post", &id_list(id))
            .map_err(cron_error)?;
        Ok(())
    }

    /// `_future_post_hook( $deprecated, $post )` for the post `id` dated `post_date`: `publish_future_post`
    /// unscheduled, then scheduled at `strtotime( get_gmt_from_date( $post->post_date ) . ' GMT' )` (`false` where
    /// that fails, which the cron function refuses).
    pub fn future_post_hook(&self, id: PostId, post_date: &[u8]) -> Result<(), PostError> {
        let cron = &*self.d.cron;
        cron.clear_scheduled_hook("publish_future_post", &id_list(id))
            .map_err(cron_error)?;
        let mut gmt = self.d.clock.gmt_from_date(post_date);
        gmt.extend_from_slice(b" GMT");
        let timestamp = self
            .d
            .clock
            .timestamp(&gmt)
            .map_or(HookValue::Bool(false), HookValue::Int);
        cron.schedule_single_event(&timestamp, "publish_future_post", &id_list(id))
            .map_err(cron_error)?;
        Ok(())
    }

    /// `_publish_post_hook( $post_id )`: `xmlrpc_publish_post` in an XML-RPC request; nothing more while importing;
    /// otherwise `_pingme` (when `default_pingback_flag` is on) and `_encloseme`, `_trackbackme` for a post with
    /// URLs to ping, and `do_pings` scheduled now unless it already is.
    pub fn publish_post_hook(&self, id: i64) -> Result<(), PostError> {
        if self.d.runtime.defined("XMLRPC_REQUEST") {
            self.action("xmlrpc_publish_post", || vec![HookValue::Int(id)])?;
        }
        if self.d.runtime.defined("WP_IMPORTING") {
            return Ok(());
        }
        let post = post_id(id);
        if !is_empty(&self.option("default_pingback_flag")?) {
            self.add_meta(post, b"_pingme", s("1"), true)?;
        }
        self.add_meta(post, b"_encloseme", s("1"), true)?;
        let to_ping = match id {
            0 => self.d.posts.get_to_ping(PostRef::Current)?,
            id if id > 0 => self.d.posts.get_to_ping(PostRef::Id(post))?,
            _ => PhpValue::Bool(false),
        };
        if !is_empty(&to_ping) {
            self.add_meta(post, b"_trackbackme", s("1"), false)?;
        }
        let cron = &*self.d.cron;
        let empty = HookValue::Array(Vec::new());
        let next = cron
            .next_scheduled("do_pings", &empty)
            .map_err(cron_error)?;
        if is_empty(&pv(&next)?) {
            let now = self.d.clock.now_gmt();
            let time = self.d.clock.timestamp(now.as_bytes()).unwrap_or(0);
            cron.schedule_single_event(&HookValue::Int(time), "do_pings", &empty)
                .map_err(cron_error)?;
        }
        Ok(())
    }

    /// `wp_check_post_hierarchy_for_loops( $post_parent, $post_id )`, the `wp_insert_post_parent` default callback:
    /// no parent is 0; a new post keeps its parent; a post cannot be its own parent; a parent that closes a loop
    /// through the post is 0; a loop the post is not on is broken with `wp_update_post( array( 'ID' => $member,
    /// 'post_parent' => 0 ) )` for each member, in order (a real re-entry), and the post keeps its parent.
    ///
    /// The answer is the parent as given wherever WordPress returns it as given. A known divergence for a parent or
    /// id that is not an int: the loop walk reads it with `(int)`, where PHP's `$override` lookup keys it as an array
    /// key; a negative parent names no post, so it closes no loop.
    pub fn check_post_hierarchy_for_loops_filter(
        &self,
        parent: PhpValue,
        id: PhpValue,
    ) -> Result<PhpValue, PostError> {
        if is_empty(&parent) {
            return Ok(PhpValue::Int(0));
        }
        if is_empty(&id) {
            return Ok(parent);
        }
        if identical(&parent, &id) {
            return Ok(PhpValue::Int(0));
        }
        let (Ok(parent_id), Ok(own)) = (
            u64::try_from(to_php_int(&parent)),
            u64::try_from(to_php_int(&id)),
        ) else {
            return Ok(parent);
        };
        let check =
            check_post_hierarchy_for_loops(PostId(parent_id), PostId(own), &Reader(&self.d.posts))?;
        if check.parent.0 == 0 {
            return Ok(PhpValue::Int(0));
        }
        for member in check.break_loop {
            let postarr = PhpValue::Array(vec![
                (key("ID"), PhpValue::Int(int(member))),
                (key("post_parent"), PhpValue::Int(0)),
            ]);
            // WordPress ignores what each call returns.
            let _ = self.wp_update_post(postarr, false, true)?;
        }
        Ok(parent)
    }
}

/// A callback that runs `run` on the API while it lives (a dropped API leaves the hook a no-op answering its first
/// argument).
fn callback(
    id: &str,
    api: Weak<PostWritesApi>,
    run: impl Fn(&PostWritesApi, &[HookValue]) -> Result<HookValue, PostError> + 'static,
) -> Callback {
    Callback::rust(id, move |args: &[HookValue]| {
        let Some(api) = api.upgrade() else {
            return Ok(args.first().cloned().unwrap_or(HookValue::Null));
        };
        run(&api, args).map_err(|e| match e {
            PostError::Hook(e) => e,
            other => HookError::message(other.to_string()),
        })
    })
}

/// The object argument a callback received, read through the port.
fn object_arg(
    api: &PostWritesApi,
    args: &[HookValue],
    at_index: usize,
) -> Result<Option<Vec<(PhpKey, PhpValue)>>, PostError> {
    match args.get(at_index) {
        Some(value) => api.d.objects.read(value),
        None => Ok(None),
    }
}

/// `_future_post_hook`'s callback, which `WP_Post_Type::add_hooks()` adds to `future_{$post_type}` at priority 5
/// with 2 arguments ([`register_future_post_hook`]).
pub fn future_post_hook_callback(api: &Rc<PostWritesApi>) -> Callback {
    callback("_future_post_hook", Rc::downgrade(api), |api, args| {
        if let Some(properties) = object_arg(api, args, 1)? {
            let (id, _) = id_and_type(&properties);
            let date = at(&properties, "post_date")
                .map(to_php_string)
                .unwrap_or_default();
            api.future_post_hook(id, &date)?;
        }
        Ok(HookValue::Null)
    })
}

/// `add_action( "future_{$post_type}", '_future_post_hook', 5, 2 )`, as `WP_Post_Type::add_hooks()` registers it.
pub fn register_future_post_hook(
    hooks: &dyn HookApi,
    api: &Rc<PostWritesApi>,
    post_type: &PostTypeName,
) -> Result<(), PostError> {
    hooks.add_filter(
        &crate::hook::byte_name(format!("future_{post_type}").into_bytes()),
        future_post_hook_callback(api),
        &HookValue::Int(5),
        2,
    )?;
    Ok(())
}

/// Registers the write use cases' default callbacks as default-filters.php does, in its order:
/// `wp_check_post_hierarchy_for_loops` on `wp_insert_post_parent` (10, 2), `_publish_post_hook` on `publish_post`
/// (5, 1) and `_transition_post_status` on `transition_post_status` (5, 3). `_future_post_hook` is per post type
/// ([`register_future_post_hook`]).
pub fn register_post_write_callbacks(
    hooks: &dyn HookApi,
    api: &Rc<PostWritesApi>,
) -> Result<(), PostError> {
    let me = Rc::downgrade(api);
    hooks.add_filter(
        &HookName::from("wp_insert_post_parent"),
        callback(
            "wp_check_post_hierarchy_for_loops",
            me.clone(),
            |api, args| {
                let parent = args.first().map(pv).transpose()?.unwrap_or(PhpValue::Null);
                let id = args.get(1).map(pv).transpose()?.unwrap_or(PhpValue::Null);
                Ok(hv(&api.check_post_hierarchy_for_loops_filter(parent, id)?))
            },
        ),
        &HookValue::Int(10),
        2,
    )?;
    hooks.add_filter(
        &HookName::from("publish_post"),
        callback("_publish_post_hook", me.clone(), |api, args| {
            let id = args
                .first()
                .map(pv)
                .transpose()?
                .map_or(0, |v| to_php_int(&v));
            api.publish_post_hook(id)?;
            Ok(HookValue::Null)
        }),
        &HookValue::Int(5),
        1,
    )?;
    hooks.add_filter(
        &HookName::from("transition_post_status"),
        callback("_transition_post_status", me, |api, args| {
            let new = args.first().map(text_arg).transpose()?.unwrap_or_default();
            let old = args.get(1).map(text_arg).transpose()?.unwrap_or_default();
            if let Some(properties) = object_arg(api, args, 2)? {
                let (id, post_type) = id_and_type(&properties);
                api.transition_post_status_default(&new, &old, id, &post_type)?;
            }
            Ok(HookValue::Null)
        }),
        &HookValue::Int(5),
        3,
    )?;
    Ok(())
}
