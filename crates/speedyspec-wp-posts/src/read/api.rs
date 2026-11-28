//! `PostsApi`: the post read use cases over their ports, and the helpers they share.
//!
//! Patterns: Facade (the [`Posts`] contract over one method per `post.php` function), Observer (every filter and
//! action through [`HookApi`], with the `skip_if_empty` fast path), Repository ([`PostRepository`], one statement
//! per method), Data Mapper ([`PostEntry`](crate::PostEntry) for the `posts` cache entry), Identity Map (the `posts`
//! cache group, kept as WordPress's `stdClass`), and Adapter for the outbound ports to unported code ([`Users`],
//! [`Taxonomy`], [`Formatting`], [`CurrentPost`], [`Microtime`], [`CacheInvalidation`]).

use std::rc::Rc;

use speedyspec_php_serialize::PhpValue;
use speedyspec_php_serialize::semantics::is_empty;
use speedyspec_wp_hooks_contracts::{Count, HookApi, HookError, HookName, HookValue, Key};
use speedyspec_wp_meta_contracts::{MetaType, Metadata, MetadataError};
use speedyspec_wp_object_cache_contracts::{ObjectCache, get_with};
use speedyspec_wp_options_contracts::{Options, OptionsError};
use speedyspec_wp_posts_contracts::{
    CurrentPost, Filter, MimeFilter, MimeType, Post, PostError, PostId, PostRef, PostRepository,
    PostStatusName, PostStatuses, PostTypeArg, PostTypeName, PostTypes, Posts,
};

use crate::hook::{filter, hv, pv};
use crate::read::ports::{CacheInvalidation, Microtime, Taxonomy, Users};
use crate::{Formatting, SanitizePorts};

/// The `posts` entries found plain raw most recently, by their shared handle (a small Identity Map of a verdict).
///
/// An entry carried as a `PhpValue` is immutable behind its handle, so the verdict holds for as long as the handle
/// lives. A kept weak handle pins the address, so an address that matches is the same entry, never a new one at a
/// freed entry's address.
#[derive(Default)]
pub(crate) struct PlainEntries {
    seen: std::cell::RefCell<[Option<std::rc::Weak<dyn std::any::Any>>; PlainEntries::SLOTS]>,
    next: std::cell::Cell<usize>,
}

impl PlainEntries {
    const SLOTS: usize = 16;

    /// Whether `entry` is plain raw, checked once per entry while it stays among the last [`Self::SLOTS`] found.
    pub(crate) fn check(&self, entry: &HookValue) -> bool {
        let HookValue::Opaque(handle) = entry else {
            return false;
        };
        let at = Rc::as_ptr(handle).cast::<()>();
        if self
            .seen
            .borrow()
            .iter()
            .flatten()
            .any(|seen| std::ptr::eq(seen.as_ptr().cast::<()>(), at))
        {
            return true;
        }
        if !crate::PostEntry::is_plain_raw(entry) {
            return false;
        }
        let slot = self.next.get();
        self.seen.borrow_mut()[slot] = Some(Rc::downgrade(handle));
        self.next.set((slot + 1) % Self::SLOTS);
        true
    }
}

/// What the post read use cases work through. The runtime passes the concrete adapters in.
#[derive(Clone)]
pub struct PostsDeps {
    pub hooks: Rc<dyn HookApi>,
    /// The request's current object cache.
    pub cache: Rc<dyn ObjectCache>,
    /// `wp_posts`.
    pub repository: Rc<dyn PostRepository>,
    /// The metadata API: `update_postmeta_cache()`, `get_post_meta()` and `metadata_exists()`.
    pub metadata: Rc<dyn Metadata>,
    /// `$wp_post_types`, read live.
    pub types: Rc<dyn PostTypes>,
    /// `$wp_post_statuses`, read live.
    pub statuses: Rc<dyn PostStatuses>,
    /// The global `$post`.
    pub current: Rc<dyn CurrentPost>,
    /// `sanitize_post_field()`'s escaping.
    pub formatting: Rc<dyn Formatting>,
    /// `is_user_logged_in()`, `get_current_user_id()` and `current_user_can()`.
    pub users: Rc<dyn Users>,
    /// `update_object_term_cache()`, `clean_object_term_cache()` and `WP_Post`'s term properties.
    pub terms: Rc<dyn Taxonomy>,
    /// `microtime()`, for `wp_cache_set_last_changed()`.
    pub clock: Rc<dyn Microtime>,
    /// `$_wp_suspend_cache_invalidation`.
    pub invalidation: Rc<dyn CacheInvalidation>,
    /// The options API (the options contracts' inbound port): `get_option()`, for `is_sticky()`.
    pub options: Rc<dyn Options>,
}

/// WordPress's post reading (`post.php` and `WP_Post::get_instance()`), one method per function. It implements the
/// [`Posts`] contract and handles every post read message ([`register_posts_handlers`](crate::register_posts_handlers)).
pub struct PostsApi {
    pub(crate) d: PostsDeps,
    /// The `posts` entries last found plain raw ([`PostEntry::is_plain_raw`](crate::PostEntry::is_plain_raw)).
    pub(crate) plain: PlainEntries,
}

impl PostsApi {
    pub fn new(deps: PostsDeps) -> Rc<PostsApi> {
        Rc::new(PostsApi {
            d: deps,
            plain: PlainEntries::default(),
        })
    }

    pub fn deps(&self) -> &PostsDeps {
        &self.d
    }

    /// The sanitize functions' ports.
    pub(crate) fn sanitize_ports(&self) -> SanitizePorts<'_> {
        SanitizePorts {
            hooks: &*self.d.hooks,
            formatting: &*self.d.formatting,
        }
    }

    /// `apply_filters()` on a PHP value, through the fast path when nothing listens.
    pub(crate) fn filter(
        &self,
        hook: &str,
        value: PhpValue,
        args: impl FnOnce() -> Vec<HookValue>,
    ) -> Result<PhpValue, PostError> {
        filter(&*self.d.hooks, &HookName::from(hook), value, args)
    }

    /// `do_action()`, through the fast path when nothing listens.
    pub(crate) fn action(
        &self,
        hook: &str,
        args: impl FnOnce() -> Vec<HookValue>,
    ) -> Result<(), PostError> {
        let name = HookName::from(hook);
        if self.d.hooks.skip_if_empty(&name, Count::Actions)? {
            return Ok(());
        }
        Ok(self.d.hooks.do_action(&name, args())?)
    }

    /// `wp_cache_get( $key, $group )` as a PHP value; `None` on a miss.
    pub(crate) fn cache_get(
        &self,
        key: &HookValue,
        group: &str,
    ) -> Result<Option<PhpValue>, PostError> {
        let group = HookValue::Str(group.to_owned());
        get_with(&*self.d.cache, key, &group, false, |v| {
            v.map(pv).transpose()
        })?
    }

    /// `wp_cache_get( $key, $group )` with the cached value lent to `f` (`None` on a miss), as the object cache
    /// holds it: nothing is copied unless `f` copies it.
    pub(crate) fn cache_read<T>(
        &self,
        key: &HookValue,
        group: &str,
        f: impl FnOnce(Option<&HookValue>) -> Result<T, PostError>,
    ) -> Result<T, PostError> {
        let group = HookValue::Str(group.to_owned());
        get_with(&*self.d.cache, key, &group, false, f)?
    }

    /// `wp_cache_set( $key, $value, $group )`.
    pub(crate) fn cache_set(
        &self,
        key: &HookValue,
        value: &PhpValue,
        group: &str,
    ) -> Result<(), PostError> {
        self.d
            .cache
            .set(key, hv(value), &HookValue::Str(group.to_owned()), 0)?;
        Ok(())
    }

    /// `wp_cache_add( $key, $value, $group )`.
    pub(crate) fn cache_add(
        &self,
        key: &HookValue,
        value: &PhpValue,
        group: &str,
    ) -> Result<(), PostError> {
        self.d
            .cache
            .add(key, hv(value), &HookValue::Str(group.to_owned()), 0)?;
        Ok(())
    }

    /// `wp_cache_delete( $key, $group )`.
    pub(crate) fn cache_delete(&self, key: &HookValue, group: &str) -> Result<(), PostError> {
        self.d
            .cache
            .delete(key, &HookValue::Str(group.to_owned()))?;
        Ok(())
    }

    /// `wp_cache_get_multiple( $keys, $group )`: each key with its value, `false` for a miss.
    pub(crate) fn cache_get_multiple(
        &self,
        keys: &[HookValue],
        group: &str,
    ) -> Result<Vec<(Key, PhpValue)>, PostError> {
        self.d
            .cache
            .get_multiple(keys, &HookValue::Str(group.to_owned()), false)?
            .into_iter()
            .map(|(k, v)| Ok((k, pv(&v)?)))
            .collect()
    }

    /// `wp_cache_add_multiple( $data, $group )`.
    pub(crate) fn cache_add_multiple(
        &self,
        data: Vec<(Key, PhpValue)>,
        group: &str,
    ) -> Result<(), PostError> {
        let data = data.into_iter().map(|(k, v)| (k, hv(&v))).collect();
        self.d
            .cache
            .add_multiple(data, &HookValue::Str(group.to_owned()), 0)?;
        Ok(())
    }

    /// The `post` meta type.
    pub(crate) fn post_meta() -> MetaType {
        MetaType::new("post").expect("post is a meta type")
    }
}

/// A metadata failure as a post failure: a callback's payload travels whole.
pub(crate) fn meta_error(e: MetadataError) -> PostError {
    match e {
        MetadataError::Hook(e) => PostError::Hook(e),
        MetadataError::Cache(e) => PostError::Cache(e),
        MetadataError::Db(e) => PostError::Db(e),
    }
}

/// What a PHP failure's payload is while PHP unwinds (an `exit()` or a fatal error): no exception to rethrow.
struct Unwinding;

/// An options failure as a post failure: a callback's payload travels whole, and a PHP exception an options adapter
/// met becomes the hook error's payload, as the posts adapters carry theirs.
pub(crate) fn options_error(e: OptionsError) -> PostError {
    match e {
        OptionsError::Hook(e) => PostError::Hook(e),
        OptionsError::Cache(e) => PostError::Cache(e),
        OptionsError::Db(e) => PostError::Db(e),
        OptionsError::Php(exception) => PostError::Hook(HookError {
            message: "a PHP function the options API called failed".into(),
            payload: Some(if matches!(exception, HookValue::Null) {
                Rc::new(Unwinding) as Rc<dyn std::any::Any>
            } else {
                Rc::new(exception)
            }),
        }),
        other => PostError::Hook(HookError::message(other.to_string())),
    }
}

/// A PHP int for an id (`PHP_INT_MAX` above it).
pub(crate) fn int(id: u64) -> i64 {
    i64::try_from(id).unwrap_or(i64::MAX)
}

/// A byte string as PHP's string.
pub(crate) fn s(bytes: impl AsRef<[u8]>) -> PhpValue {
    PhpValue::Str(bytes.as_ref().to_vec())
}

/// PHP's truthiness.
pub(crate) fn truthy(value: &PhpValue) -> bool {
    !is_empty(value)
}

/// `$value === 'text'`.
pub(crate) fn is_str(value: &PhpValue, text: &str) -> bool {
    matches!(value, PhpValue::Str(b) if b.as_slice() == text.as_bytes())
}

/// `$object->$name` on an object value; `None` for a missing property or a non-object.
pub(crate) fn prop<'a>(value: &'a PhpValue, name: &str) -> Option<&'a PhpValue> {
    match value {
        PhpValue::Object { properties, .. } => {
            speedyspec_php_serialize::semantics::get(properties, name)
        }
        _ => None,
    }
}

/// `$value instanceof stdClass`, by class name (no class hierarchy is known).
pub(crate) fn is_std_class(value: &PhpValue) -> bool {
    matches!(value, PhpValue::Object { class, .. } if class.eq_ignore_ascii_case("stdClass"))
}

/// A `false` result where WordPress returns false.
pub(crate) fn none_if_false(value: PhpValue) -> Option<PhpValue> {
    match value {
        PhpValue::Bool(false) => None,
        other => Some(other),
    }
}

/// Which `Posts` read a native caller is about to make, for [`read_hooks`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PostRead<'a> {
    /// `get_post()` with the `raw` filter (another context fires the `sanitize_post_field()` filters, which
    /// [`PostRead::Field`] lists).
    Get,
    /// `get_post_field( $name, $post, $context )`.
    Field { name: &'a [u8], context: &'a str },
    /// `get_post_status()`.
    Status,
    /// `get_post_type()`.
    PostType,
    /// `get_post_mime_type()`.
    MimeType,
    /// `get_post_ancestors()`.
    Ancestors,
    /// `wp_get_post_parent_id()`.
    ParentId,
    /// `get_page_by_path()`.
    PageByPath,
    /// `get_page_uri()`.
    PageUri,
    /// `get_all_page_ids()`.
    AllPageIds,
    /// `wp_count_posts()`.
    CountPosts,
    /// `wp_count_attachments()`.
    CountAttachments,
    /// `get_lastpostdate()`.
    LastPostDate,
    /// `get_lastpostmodified()`.
    LastPostModified,
    /// `get_available_post_mime_types()`.
    AvailableMimeTypes,
    /// `update_post_cache()`.
    UpdateCache,
    /// `clean_post_cache()`.
    CleanCache,
    /// `_prime_post_caches( $ids, $terms, $meta )`.
    PrimeCaches { meta: bool },
    /// `_prime_post_parent_id_caches()`.
    PrimeParentCaches,
    /// `update_post_caches( $posts, $post_type, $terms, $meta )`.
    UpdatePostCaches { meta: bool },
    /// `update_post_parent_caches()`, which primes the parents with their meta.
    UpdatePostParentCaches,
}

/// The hooks a post meta read (`get_post_meta()`, `metadata_exists()`) may fire, as the metadata seam's
/// `read_hooks( 'post' )` lists them: the short-circuit, the cold-cache `update_meta_cache()` short-circuit, and the
/// default.
const META_READ: [&str; 3] = [
    "get_post_metadata",
    "update_post_metadata_cache",
    "default_post_metadata",
];

/// The hooks a read may fire, for the native-path decision (spec, *Native paths*): when none has a PHP callback,
/// the read can stay in Rust. `get_post()` and the cache reads fire none; the `posts` group has no hooks.
///
/// The list names this seam's hooks and the metadata hooks every read that can reach post meta fires on a cold
/// `post_meta` entry (`get_post_metadata`, `update_post_metadata_cache`, `default_post_metadata`), plus the escaping hooks of the [`Formatting`] port
/// (`format_to_edit`, `attribute_escape`, `js_escape`, `user_can_richedit`). The [`Users`] and [`Taxonomy`] ports'
/// own hooks (`user_has_cap`, `map_meta_cap`, the term cache hooks) and `$wpdb`'s `query` are the binding's to add.
pub fn read_hooks(read: PostRead<'_>) -> Vec<HookName> {
    let names: Vec<Vec<u8>> = match read {
        PostRead::Get
        | PostRead::PostType
        | PostRead::MimeType
        | PostRead::Ancestors
        | PostRead::ParentId
        | PostRead::AllPageIds
        | PostRead::UpdateCache
        | PostRead::PrimeParentCaches => Vec::new(),
        PostRead::Field { name, context } => {
            let mut hooks = Vec::new();
            if name == b"page_template" || (!crate::PostEntry::is_declared(name) && !is_magic(name))
            {
                hooks.extend(words(&META_READ));
            }
            hooks.extend(crate::sanitize::field_hooks(name, context));
            hooks
        }
        PostRead::Status => [words(&META_READ), words(&["get_post_status"])].concat(),
        PostRead::PageByPath => words(&["wp_cache_set_last_changed", "sanitize_title"]),
        PostRead::PageUri => words(&["get_page_uri"]),
        PostRead::CountPosts => words(&["pre_wp_count_posts", "wp_count_posts"]),
        PostRead::CountAttachments => words(&["pre_wp_count_attachments", "wp_count_attachments"]),
        PostRead::LastPostDate => words(&["sanitize_key", "get_lastpostdate"]),
        PostRead::LastPostModified => words(&[
            "pre_get_lastpostmodified",
            "sanitize_key",
            "get_lastpostdate",
            "get_lastpostmodified",
        ]),
        PostRead::AvailableMimeTypes => words(&["pre_get_available_post_mime_types"]),
        PostRead::CleanCache => words(&[
            "clean_post_cache",
            "clean_page_cache",
            "wp_cache_set_last_changed",
        ]),
        PostRead::UpdatePostParentCaches => words(&["update_post_metadata_cache"]),
        PostRead::PrimeCaches { meta } | PostRead::UpdatePostCaches { meta } => {
            if meta {
                words(&["update_post_metadata_cache"])
            } else {
                Vec::new()
            }
        }
    };
    names.into_iter().map(crate::hook::byte_name).collect()
}

fn words(names: &[&str]) -> Vec<Vec<u8>> {
    names.iter().map(|n| n.as_bytes().to_vec()).collect()
}

/// `WP_Post::__isset()`'s names that need no metadata lookup.
pub(crate) fn is_magic(name: &[u8]) -> bool {
    matches!(
        name,
        b"ancestors" | b"page_template" | b"post_category" | b"tags_input"
    )
}

/// The [`Posts`] contract over the use cases. Where WordPress returns a filter's answer as it is, the contract's
/// narrower types keep what they can carry; the inherent methods and the messages keep the whole answer.
impl Posts for PostsApi {
    fn get(&self, id: PostRef, _filter: Filter) -> Result<Option<Post>, PostError> {
        self.get_post(id)
    }

    fn field(&self, name: &str, id: PostRef, filter: Filter) -> Result<PhpValue, PostError> {
        self.get_post_field(name.as_bytes(), id, filter.as_str())
    }

    fn status(&self, id: PostRef) -> Result<Option<PostStatusName>, PostError> {
        Ok(none_if_false(self.get_post_status(id)?).map(|v| {
            PostStatusName::new(String::from_utf8_lossy(
                &speedyspec_php_serialize::semantics::to_php_string(&v),
            ))
        }))
    }

    fn post_type(&self, id: PostRef) -> Result<Option<PostTypeName>, PostError> {
        self.get_post_type(id)
    }

    fn mime_type(&self, id: PostRef) -> Result<Option<MimeType>, PostError> {
        self.get_post_mime_type(id)
    }

    fn ancestors(&self, id: PostRef) -> Result<Vec<PostId>, PostError> {
        self.get_post_ancestors(id)
    }

    fn parent_id(&self, id: PostRef) -> Result<Option<PostId>, PostError> {
        self.wp_get_post_parent_id(id)
    }

    fn page_by_path(&self, path: &str, post_type: &PostTypeArg) -> Result<Option<Post>, PostError> {
        self.get_page_by_path(path.as_bytes(), post_type)
    }

    fn page_uri(&self, id: PostRef) -> Result<Option<Vec<u8>>, PostError> {
        Ok(none_if_false(self.get_page_uri(id)?)
            .map(|v| speedyspec_php_serialize::semantics::to_php_string(&v)))
    }

    fn all_page_ids(&self) -> Result<Vec<PostId>, PostError> {
        Ok(self
            .get_all_page_ids()?
            .iter()
            .map(|v| {
                PostId(
                    u64::try_from(speedyspec_php_serialize::semantics::to_php_int(v)).unwrap_or(0),
                )
            })
            .collect())
    }

    fn count_posts(&self, post_type: &PostTypeName, perm: &str) -> Result<PhpValue, PostError> {
        self.wp_count_posts(post_type.as_str(), perm)
    }

    fn count_attachments(&self, mime_type: Option<&MimeFilter>) -> Result<PhpValue, PostError> {
        self.wp_count_attachments(mime_type)
    }

    fn last_post_date(
        &self,
        timezone: &str,
        post_type: &PostTypeName,
    ) -> Result<PhpValue, PostError> {
        self.get_lastpostdate(timezone, post_type.as_str())
    }

    fn last_post_modified(
        &self,
        timezone: &str,
        post_type: &PostTypeName,
    ) -> Result<PhpValue, PostError> {
        self.get_lastpostmodified(timezone, post_type.as_str())
    }

    fn available_mime_types(&self, post_type: &PostTypeName) -> Result<Vec<MimeType>, PostError> {
        Ok(self
            .get_available_post_mime_types(post_type.as_str())?
            .iter()
            .map(|v| MimeType::new(speedyspec_php_serialize::semantics::to_php_string(v)))
            .collect())
    }

    fn update_cache(&self, posts: &[Post]) -> Result<(), PostError> {
        self.update_post_cache(posts)
    }

    fn clean_cache(&self, id: PostId) -> Result<(), PostError> {
        self.clean_post_cache(PostRef::Id(id))
    }

    fn prime_caches(&self, ids: &[PostId], terms: bool, meta: bool) -> Result<(), PostError> {
        self.prime_post_caches(ids, terms, meta)
    }

    fn prime_parent_caches(&self, ids: &[PostId]) -> Result<(), PostError> {
        self.prime_post_parent_id_caches(ids)
    }
}
