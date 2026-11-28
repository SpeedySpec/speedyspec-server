//! The outbound ports the write use cases call beyond the contracts' (Adapter): the formatting functions
//! `wp_insert_post()` calls, the cron functions, the PHP objects the hooks receive, the slug settings
//! `wp_unique_post_slug()` reads, and the runtime's constants and notices.
//!
//! The PHP binding fills each with WordPress's own code until the seam behind it is ported; Rust adapters replace them
//! as each seam lands, and PHP may replace any of them. A callback's failure comes back as [`PostError::Hook`], and
//! the use case stops there, as WordPress would.

use speedyspec_php_serialize::{PhpKey, PhpValue};
use speedyspec_wp_cron_contracts::{CronError, CronResult};
use speedyspec_wp_hooks_contracts::{HookApi, HookError, HookName, HookValue};
use speedyspec_wp_posts_contracts::{BadSlugs, PostError, PostObject, PostParent};

use crate::PostEntry;
use crate::SlugContext;
use crate::hook::{bytes_value, filter_hook, hv, pv};

/// The formatting functions `wp_insert_post()` calls (formatting.php), each with its own filters.
pub trait WriteFormatting {
    /// `sanitize_title( $title, $fallback_title, $context )`: `remove_accents()` for the `save` context, the
    /// `sanitize_title` filter (whose default callback is `sanitize_title_with_dashes()`), then the fallback for an
    /// empty answer.
    fn sanitize_title(
        &self,
        title: &PhpValue,
        fallback: &PhpValue,
        context: &str,
    ) -> Result<PhpValue, PostError>;
    /// `wp_encode_emoji( $content )`, for a column whose charset is `utf8` or `utf8mb3`.
    fn encode_emoji(&self, content: &PhpValue) -> Result<PhpValue, PostError>;
}

/// The cron functions the post functions call (cron.php). The Rust adapter is `speedyspec-wp-cron`'s scheduling core
/// over the cron contracts; the arguments and answers are the contracts' own.
pub trait Scheduling {
    /// `wp_clear_scheduled_hook( $hook, $args )`.
    fn clear_scheduled_hook(&self, hook: &str, args: &HookValue) -> Result<CronResult, CronError>;
    /// `wp_schedule_single_event( $timestamp, $hook, $args )`; `timestamp` as PHP holds it (`false` where
    /// `strtotime()` failed).
    fn schedule_single_event(
        &self,
        timestamp: &HookValue,
        hook: &str,
        args: &HookValue,
    ) -> Result<CronResult, CronError>;
    /// `wp_next_scheduled( $hook, $args )`: the timestamp, or `false`.
    fn next_scheduled(&self, hook: &str, args: &HookValue) -> Result<HookValue, CronError>;
}

/// The PHP objects the post hooks receive (Adapter): `new WP_Post()` for a post with no object yet, and reading an
/// object a callback may have changed.
///
/// The PHP binding builds a real `WP_Post` and reads its properties live, so every hook shares one instance;
/// [`PlainPostObjects`] is the Rust adapter, which carries the object as a value.
pub trait PostObjects {
    /// The object for `post`: its handle when it has one, else a new `WP_Post` built from `post.post` with the
    /// dynamic properties set on it in order.
    fn handle(&self, post: &PostObject) -> Result<HookValue, PostError>;
    /// The object's properties as PHP holds them now; `None` for a value that is not an object.
    fn read(&self, handle: &HookValue) -> Result<Option<Vec<(PhpKey, PhpValue)>>, PostError>;
}

/// [`PostObjects`] in Rust: a `WP_Post` value (`PhpValue::Object`) carried opaque, the same `Rc` to every hook.
pub struct PlainPostObjects;

impl PostObjects for PlainPostObjects {
    fn handle(&self, post: &PostObject) -> Result<HookValue, PostError> {
        if let Some(handle) = &post.handle {
            return Ok(handle.clone());
        }
        let mut properties = PostEntry::raw_properties(&post.post);
        for (name, value) in &post.properties {
            let value = pv(value)?;
            let key = PhpKey::Str(name.as_bytes().to_vec());
            match properties.iter_mut().find(|(k, _)| *k == key) {
                Some(slot) => slot.1 = value,
                None => properties.push((key, value)),
            }
        }
        Ok(hv(&PostEntry::wp_post(properties)))
    }

    fn read(&self, handle: &HookValue) -> Result<Option<Vec<(PhpKey, PhpValue)>>, PostError> {
        Ok(match pv(handle)? {
            PhpValue::Object { properties, .. } => Some(properties),
            _ => None,
        })
    }
}

/// What `wp_unique_post_slug()` reads from `$wp_rewrite` and the options: the feeds, the pagination base and the
/// permalink structure.
pub trait SlugSettings {
    /// The settings as they are now.
    fn slug_context(&self) -> SlugContext;
}

/// The request's constants and notices.
pub trait Runtime {
    /// `defined( $name )`: `XMLRPC_REQUEST` and `WP_IMPORTING` for `_publish_post_hook()`.
    fn defined(&self, name: &str) -> bool;
    /// `_doing_it_wrong( $function, $message, $version )`, with its hooks; `message` is already translated.
    fn doing_it_wrong(&self, function: &str, message: &str, version: &str)
    -> Result<(), PostError>;
}

/// A cron failure as a post failure: a callback's payload travels whole.
pub(crate) fn cron_error(e: CronError) -> PostError {
    PostError::Hook(HookError {
        message: e.message,
        payload: e.payload,
    })
}

/// The `wp_unique_post_slug_is_bad_*_slug` filters through the Hook API (Observer behind the [`BadSlugs`]
/// Strategy). A callback's failure is the answer, so the use case stops before the `wp_unique_post_slug` filter, as
/// WordPress's exception would.
pub(crate) struct HookBadSlugs<'a> {
    hooks: &'a dyn HookApi,
}

impl<'a> HookBadSlugs<'a> {
    pub(crate) fn new(hooks: &'a dyn HookApi) -> Self {
        Self { hooks }
    }

    fn run(&self, hook: &str, args: impl FnOnce() -> Vec<HookValue>) -> Result<bool, PostError> {
        let answer = filter_hook(
            self.hooks,
            &HookName::from(hook),
            HookValue::Bool(false),
            args,
        )?;
        Ok(!speedyspec_php_serialize::semantics::is_empty(&pv(
            &answer,
        )?))
    }
}

impl BadSlugs for HookBadSlugs<'_> {
    fn bad_attachment_slug(&self, slug: &[u8]) -> Result<bool, PostError> {
        self.run("wp_unique_post_slug_is_bad_attachment_slug", || {
            vec![bytes_value(slug)]
        })
    }

    fn bad_hierarchical_slug(
        &self,
        slug: &[u8],
        post_type: &str,
        parent: PostParent,
    ) -> Result<bool, PostError> {
        self.run("wp_unique_post_slug_is_bad_hierarchical_slug", || {
            vec![
                bytes_value(slug),
                HookValue::Str(post_type.to_owned()),
                match parent {
                    PostParent::Int(n) => HookValue::Int(n),
                    PostParent::Null => HookValue::Null,
                },
            ]
        })
    }

    fn bad_flat_slug(&self, slug: &[u8], post_type: &str) -> Result<bool, PostError> {
        self.run("wp_unique_post_slug_is_bad_flat_slug", || {
            vec![bytes_value(slug), HookValue::Str(post_type.to_owned())]
        })
    }
}
