//! The post read use cases' outbound ports over PHP (Adapter), kept until the seams that own users, taxonomy and
//! formatting bind Rust ones, and the decorators that put the request's PHP values at the edge of the ports PHP code
//! shares with the use cases (the object cache, the post repository binding).
//!
//! A port whose contract returns `Result` answers a PHP exception as [`PostError::Hook`] with the exception as its
//! payload, so the use case stops and the native function rethrows the same object. A port whose contract cannot fail
//! ([`PhpCurrentPost`], [`PhpCacheInvalidation`]) parks it ([`park`]), and the native function rethrows what was
//! parked.

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use speedyspec_container::Container;
use speedyspec_php_ext::{Key, Value};
use speedyspec_php_serialize::PhpValue;
use speedyspec_wp_hooks_contracts::{HookError, HookValue};
use speedyspec_wp_object_cache_contracts::{ObjectCache, ObjectCacheError};
use speedyspec_wp_posts::{CacheInvalidation, Formatting, Microtime, Taxonomy, Users};
use speedyspec_wp_posts_contracts::{
    CurrentPost, LastPostField, MimeFilter, MimeType, PageRow, Post, PostDateTime, PostError,
    PostId, PostRepository, PostStatusName, PostTypeName, Slug, Timezone,
};

use crate::adapters::{exists, is_parked, park, truthy};
use crate::codec::{Thrown, cache_out, copy_php, helper, php, to_php};

/// A PHP failure as the use cases see it: the exception travels whole as the hook error's payload, which the
/// native function rethrows.
pub(crate) fn thrown_error(thrown: Thrown) -> PostError {
    PostError::Hook(HookError {
        message: "a PHP function the posts API called failed".into(),
        payload: thrown
            .0
            .map(|exception| Rc::new(exception) as Rc<dyn std::any::Any>),
    })
}

/// Calls a PHP function for a port.
fn call(name: &str, args: &[Value]) -> Result<Value, PostError> {
    php(name, args).map_err(thrown_error)
}

fn into_php(value: &PhpValue) -> Result<Value, PostError> {
    to_php(value).map_err(thrown_error)
}

fn from_answer(value: &Value) -> Result<PhpValue, PostError> {
    copy_php(value).map_err(thrown_error)
}

fn id_list(ids: &[PostId]) -> Value {
    Value::Array(
        ids.iter()
            .enumerate()
            .map(|(i, id)| {
                (
                    Key::Int(i as i64),
                    Value::Int(i64::try_from(id.0).unwrap_or(i64::MAX)),
                )
            })
            .collect(),
    )
}

/// The global `$post` (`$GLOBALS['post']`), copied with its class and properties, or the object a native function
/// was given in its place.
///
/// WordPress's readers take `$post` as an id or as an object. The [`PostRef`](speedyspec_wp_posts_contracts::PostRef)
/// the use cases take names an id or the current post (a `PostRef` cannot carry an object; a note for the next
/// posts contracts), so a native function given an object hands it over for one dispatch and asks for
/// [`PostRef::Current`](speedyspec_wp_posts_contracts::PostRef::Current): the first read takes it, exactly as
/// `get_post()` would take it, and later reads see the global again.
///
/// The hand-over is scoped to the dispatch (`replace`): the native functions save what is pending, put their own
/// object (or nothing) in its place, and restore the saved value when the handler returns. A handler PHP rebinds
/// that never reads the object therefore cannot leak it into a read nested inside it.
#[derive(Default)]
pub struct PhpCurrentPost {
    given: RefCell<Option<PhpValue>>,
}

impl PhpCurrentPost {
    pub fn new() -> PhpCurrentPost {
        PhpCurrentPost::default()
    }

    /// Puts `given` in place of the pending object, which it returns.
    pub(crate) fn replace(&self, given: Option<PhpValue>) -> Option<PhpValue> {
        std::mem::replace(&mut *self.given.borrow_mut(), given)
    }
}

impl CurrentPost for PhpCurrentPost {
    fn current(&self) -> Option<PhpValue> {
        if let Some(given) = self.given.borrow_mut().take() {
            return Some(given);
        }
        if is_parked() {
            return None;
        }
        let global = speedyspec_php::global("post")?;
        match copy_php(&global) {
            Ok(PhpValue::Null) => None,
            Ok(post) => Some(post),
            Err(thrown) => {
                park(thrown);
                None
            }
        }
    }
}

/// `format_to_edit()`, `esc_attr()`, `esc_js()` and `user_can_richedit()` (formatting.php, general-template.php),
/// each with its own filters. A function that is not declared leaves the value as it is (`user_can_richedit()`:
/// false).
pub struct PhpFormatting;

impl PhpFormatting {
    fn text(&self, name: &str, args: &[Value], value: PhpValue) -> Result<PhpValue, PostError> {
        if !exists(name) {
            return Ok(value);
        }
        from_answer(&call(name, args)?)
    }
}

impl Formatting for PhpFormatting {
    fn format_to_edit(&self, content: PhpValue, rich_text: bool) -> Result<PhpValue, PostError> {
        let args = [into_php(&content)?, Value::Bool(rich_text)];
        self.text("format_to_edit", &args, content)
    }

    fn esc_attr(&self, text: PhpValue) -> Result<PhpValue, PostError> {
        let args = [into_php(&text)?];
        self.text("esc_attr", &args, text)
    }

    fn esc_js(&self, text: PhpValue) -> Result<PhpValue, PostError> {
        let args = [into_php(&text)?];
        self.text("esc_js", &args, text)
    }

    fn user_can_richedit(&self) -> Result<bool, PostError> {
        if !exists("user_can_richedit") {
            return Ok(false);
        }
        Ok(truthy(&call("user_can_richedit", &[])?))
    }

    fn sanitize_trackback_urls(&self, to_ping: PhpValue) -> Result<PhpValue, PostError> {
        let args = [into_php(&to_ping)?];
        self.text("sanitize_trackback_urls", &args, to_ping)
    }
}

/// `is_user_logged_in()`, `get_current_user_id()` and `current_user_can()` (pluggable.php, user.php,
/// capabilities.php): nobody is logged in while they are not declared.
pub struct PhpUsers;

impl Users for PhpUsers {
    fn is_user_logged_in(&self) -> Result<bool, PostError> {
        if !exists("is_user_logged_in") {
            return Ok(false);
        }
        Ok(truthy(&call("is_user_logged_in", &[])?))
    }

    fn get_current_user_id(&self) -> Result<u64, PostError> {
        if !exists("get_current_user_id") {
            return Ok(0);
        }
        Ok(match call("get_current_user_id", &[])? {
            Value::Int(i) => u64::try_from(i).unwrap_or(0),
            _ => 0,
        })
    }

    fn current_user_can(&self, capability: &PhpValue) -> Result<bool, PostError> {
        if !exists("current_user_can") {
            return Ok(false);
        }
        Ok(truthy(&call("current_user_can", &[into_php(capability)?])?))
    }
}

/// `update_object_term_cache()`, `clean_object_term_cache()` (taxonomy.php) and `WP_Post::__get()`'s term branch
/// (the facade's `term_fields()` helper). Nothing happens while taxonomy.php is not loaded.
pub struct PhpTaxonomy;

impl Taxonomy for PhpTaxonomy {
    fn update_object_term_cache(
        &self,
        ids: &[PostId],
        object_types: &[PhpValue],
    ) -> Result<(), PostError> {
        if !exists("update_object_term_cache") {
            return Ok(());
        }
        let types = Value::Array(
            object_types
                .iter()
                .enumerate()
                .map(|(i, t)| Ok((Key::Int(i as i64), into_php(t)?)))
                .collect::<Result<_, PostError>>()?,
        );
        call("update_object_term_cache", &[id_list(ids), types])?;
        Ok(())
    }

    fn clean_object_term_cache(&self, ids: &[PostId], object_type: &str) -> Result<(), PostError> {
        if !exists("clean_object_term_cache") {
            return Ok(());
        }
        call(
            "clean_object_term_cache",
            &[id_list(ids), Value::Str(object_type.into())],
        )?;
        Ok(())
    }

    fn post_term_fields(
        &self,
        post: &PhpValue,
        taxonomy: &str,
        field: &str,
    ) -> Result<PhpValue, PostError> {
        let args = [
            into_php(post)?,
            Value::Str(taxonomy.into()),
            Value::Str(field.into()),
        ];
        from_answer(&helper("term_fields", &args).map_err(thrown_error)?)
    }
}

/// PHP's `microtime()` from the system clock: `"0.12345600 1700000000"`, microseconds and two zeros, as PHP prints
/// it.
pub struct SystemMicrotime;

impl Microtime for SystemMicrotime {
    fn microtime(&self) -> String {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default();
        format!("0.{:06}00 {}", now.subsec_micros(), now.as_secs())
    }
}

/// `$_wp_suspend_cache_invalidation`, read live.
pub struct PhpCacheInvalidation;

impl CacheInvalidation for PhpCacheInvalidation {
    fn suspended(&self) -> bool {
        speedyspec_php::global("_wp_suspend_cache_invalidation").is_some_and(|v| truthy(&v))
    }
}

/// The request's object cache with the PHP objects in its answers carried as `PhpValue`s (Decorator): a `posts`
/// entry another caller stored (a `WP_Post`, a `stdClass`) is copied with its class and properties, so the use cases
/// read it as `WP_Post::get_instance()` does. Writes pass through, except while a port's failure is parked: then
/// nothing is written, as WordPress writes nothing after the exception.
pub struct PhpValueCache {
    inner: Rc<dyn ObjectCache>,
}

impl PhpValueCache {
    pub fn new(inner: Rc<dyn ObjectCache>) -> PhpValueCache {
        PhpValueCache { inner }
    }
}

/// A cached value as the use cases read it; a PHP failure is parked and reads as a miss (`false`).
fn read_out(value: &Value) -> Value {
    if is_parked() {
        return Value::Bool(false);
    }
    cache_out(value).unwrap_or_else(|thrown| {
        park(thrown);
        Value::Bool(false)
    })
}

impl ObjectCache for PhpValueCache {
    fn get(
        &self,
        key: &HookValue,
        group: &HookValue,
        force: bool,
    ) -> Result<Option<Value>, ObjectCacheError> {
        Ok(self.inner.get(key, group, force)?.map(|v| read_out(&v)))
    }

    fn read_with(
        &self,
        key: &HookValue,
        group: &HookValue,
        force: bool,
        reader: &mut dyn FnMut(Option<&Value>),
    ) -> Result<(), ObjectCacheError> {
        let mut found: Option<Option<Value>> = None;
        self.inner.read_with(key, group, force, &mut |value| {
            found = Some(value.cloned());
        })?;
        let value = found.flatten().map(|v| read_out(&v));
        reader(value.as_ref());
        Ok(())
    }

    fn get_multiple(
        &self,
        keys: &[HookValue],
        group: &HookValue,
        force: bool,
    ) -> Result<Vec<(Key, Value)>, ObjectCacheError> {
        Ok(self
            .inner
            .get_multiple(keys, group, force)?
            .into_iter()
            .map(|(k, v)| (k, read_out(&v)))
            .collect())
    }

    fn peek(&self, key: &HookValue, group: &HookValue) -> Option<Value> {
        self.inner.peek(key, group).map(|v| read_out(&v))
    }

    fn add(
        &self,
        key: &HookValue,
        data: Value,
        group: &HookValue,
        expire: i64,
    ) -> Result<bool, ObjectCacheError> {
        if is_parked() {
            return Ok(false);
        }
        self.inner.add(key, data, group, expire)
    }

    fn add_multiple(
        &self,
        data: Vec<(Key, Value)>,
        group: &HookValue,
        expire: i64,
    ) -> Result<Vec<(Key, bool)>, ObjectCacheError> {
        if is_parked() {
            return Ok(data.into_iter().map(|(k, _)| (k, false)).collect());
        }
        self.inner.add_multiple(data, group, expire)
    }

    fn set(
        &self,
        key: &HookValue,
        data: Value,
        group: &HookValue,
        expire: i64,
    ) -> Result<bool, ObjectCacheError> {
        if is_parked() {
            return Ok(false);
        }
        self.inner.set(key, data, group, expire)
    }

    fn set_multiple(
        &self,
        data: Vec<(Key, Value)>,
        group: &HookValue,
        expire: i64,
    ) -> Result<Vec<(Key, bool)>, ObjectCacheError> {
        if is_parked() {
            return Ok(data.into_iter().map(|(k, _)| (k, false)).collect());
        }
        self.inner.set_multiple(data, group, expire)
    }

    fn delete(&self, key: &HookValue, group: &HookValue) -> Result<bool, ObjectCacheError> {
        if is_parked() {
            return Ok(false);
        }
        self.inner.delete(key, group)
    }

    fn delete_multiple(
        &self,
        keys: &[HookValue],
        group: &HookValue,
    ) -> Result<Vec<(Key, bool)>, ObjectCacheError> {
        if is_parked() {
            return Ok(Vec::new());
        }
        self.inner.delete_multiple(keys, group)
    }

    fn add_global_groups(&self, groups: &[HookValue]) {
        self.inner.add_global_groups(groups)
    }

    fn is_persistent(&self) -> bool {
        self.inner.is_persistent()
    }
}

/// The container key of the taxonomy port: PHP binds an object implementing the `SpeedySpec\WpPosts\Taxonomy`
/// twin here to replace WordPress's term cache functions for the post reads.
pub const TAXONOMY: &str = "SpeedySpec\\WpPosts\\Taxonomy";

/// The taxonomy port resolved from the container on every call (as [`ResolvedRepository`] is): [`PhpTaxonomy`] is
/// the default binding; a binding that fails is [`PostError::Db`], and nothing falls back.
pub struct ResolvedTaxonomy {
    container: Weak<Container>,
    fallback: Rc<dyn Taxonomy>,
}

impl ResolvedTaxonomy {
    pub fn new(container: &Rc<Container>, fallback: Rc<dyn Taxonomy>) -> ResolvedTaxonomy {
        ResolvedTaxonomy {
            container: Rc::downgrade(container),
            fallback,
        }
    }

    fn resolved(&self) -> Result<Rc<dyn Taxonomy>, PostError> {
        let Some(container) = self.container.upgrade() else {
            return Ok(self.fallback.clone());
        };
        container
            .make_port::<dyn Taxonomy>(TAXONOMY)
            .map_err(|e| PostError::Db(format!("the taxonomy binding failed: {e}")))
    }
}

impl Taxonomy for ResolvedTaxonomy {
    fn update_object_term_cache(
        &self,
        ids: &[PostId],
        object_types: &[PhpValue],
    ) -> Result<(), PostError> {
        self.resolved()?.update_object_term_cache(ids, object_types)
    }

    fn clean_object_term_cache(&self, ids: &[PostId], object_type: &str) -> Result<(), PostError> {
        self.resolved()?.clean_object_term_cache(ids, object_type)
    }

    fn post_term_fields(
        &self,
        post: &PhpValue,
        taxonomy: &str,
        field: &str,
    ) -> Result<PhpValue, PostError> {
        self.resolved()?.post_term_fields(post, taxonomy, field)
    }
}

/// The container key of the post repository port: PHP binds an object implementing the
/// `SpeedySpec\WpPosts\PostRepository` twin here to replace `wp_posts`.
pub const POST_REPOSITORY: &str = "SpeedySpec\\WpPosts\\PostRepository";

/// The post repository resolved from the container on every call, so a binding PHP made for the request applies; the
/// default binding (the SQL table) when the container is gone. The container is held weakly: it holds this adapter's
/// owners. A binding that fails (PHP's factory threw, or its object does not implement the twin) is
/// [`PostError::Db`], and nothing falls back to the table.
pub struct ResolvedRepository {
    container: Weak<Container>,
    fallback: Rc<dyn PostRepository>,
}

impl ResolvedRepository {
    pub fn new(container: &Rc<Container>, fallback: Rc<dyn PostRepository>) -> ResolvedRepository {
        ResolvedRepository {
            container: Rc::downgrade(container),
            fallback,
        }
    }

    fn resolved(&self) -> Result<Rc<dyn PostRepository>, PostError> {
        let Some(container) = self.container.upgrade() else {
            return Ok(self.fallback.clone());
        };
        container
            .make_port::<dyn PostRepository>(POST_REPOSITORY)
            .map_err(|e| PostError::Db(format!("the post repository binding failed: {e}")))
    }
}

impl PostRepository for ResolvedRepository {
    fn engine(&self) -> String {
        self.resolved()
            .map_or_else(|_| self.fallback.engine(), |r| r.engine())
    }

    fn find(&self, id: PostId) -> Result<Option<Post>, PostError> {
        self.resolved()?.find(id)
    }

    fn insert(&self, post: &Post) -> Result<Option<PostId>, PostError> {
        self.resolved()?.insert(post)
    }

    fn attachment_slug_owner(
        &self,
        slug: &Slug,
        except: PostId,
    ) -> Result<Option<Slug>, PostError> {
        self.resolved()?.attachment_slug_owner(slug, except)
    }

    fn hierarchical_slug_owner(
        &self,
        slug: &Slug,
        post_type: &PostTypeName,
        except: PostId,
        parent: PostId,
    ) -> Result<Option<Slug>, PostError> {
        self.resolved()?
            .hierarchical_slug_owner(slug, post_type, except, parent)
    }

    fn flat_slug_owner(
        &self,
        slug: &Slug,
        post_type: &PostTypeName,
        except: PostId,
    ) -> Result<Option<Slug>, PostError> {
        self.resolved()?.flat_slug_owner(slug, post_type, except)
    }

    fn find_many(&self, ids: &[PostId]) -> Result<Vec<Post>, PostError> {
        self.resolved()?.find_many(ids)
    }

    fn parent_ids(&self, ids: &[PostId]) -> Result<Vec<(PostId, PostId)>, PostError> {
        self.resolved()?.parent_ids(ids)
    }

    fn pages_by_names(
        &self,
        names: &[Slug],
        post_types: &[PostTypeName],
    ) -> Result<Vec<PageRow>, PostError> {
        self.resolved()?.pages_by_names(names, post_types)
    }

    fn all_page_ids(&self) -> Result<Vec<PostId>, PostError> {
        self.resolved()?.all_page_ids()
    }

    fn count_by_status(
        &self,
        post_type: &PostTypeName,
        reader: Option<u64>,
    ) -> Result<Vec<(PostStatusName, u64)>, PostError> {
        self.resolved()?.count_by_status(post_type, reader)
    }

    fn count_attachments(
        &self,
        mime_type: Option<&MimeFilter>,
    ) -> Result<(Vec<(MimeType, u64)>, u64), PostError> {
        self.resolved()?.count_attachments(mime_type)
    }

    fn last_post_time(
        &self,
        field: LastPostField,
        post_types: &[PostTypeName],
        timezone: Timezone,
    ) -> Result<Option<PostDateTime>, PostError> {
        self.resolved()?.last_post_time(field, post_types, timezone)
    }

    fn available_mime_types(&self, post_type: &PostTypeName) -> Result<Vec<MimeType>, PostError> {
        self.resolved()?.available_mime_types(post_type)
    }
}
