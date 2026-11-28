//! The native post reading and post cache functions `posts.php` (the facade) calls, one per WordPress function
//! (post.php, WordPress 7.2-RC2). The use cases are dispatched through their handlers, each resolved once from the
//! container (Command/CQRS), so PHP may rebind one.
//!
//! This adapter does what the use cases leave to the PHP edge:
//!
//! - `$post` arguments: an empty one is the current post, a numeric one an id (`(int)`, nothing below 1), an object
//!   is handed to the use case in place of the global for that one read ([`PhpCurrentPost`]), anything else is no
//!   post;
//! - `get_post()` itself keeps PHP's objects: a `WP_Post` it is given (or the global) is the object it works on, so
//!   `WP_Post::filter()` (WordPress's own method) sanitizes it in place, as `get_post( 0, OBJECT, 'display' )` does
//!   to the global; a post read by id is `new WP_Post( $entry )` over the use case's raw object;
//! - `sanitize_post()` and `update_post_cache()` sanitize the caller's objects in place, as PHP does through the
//!   object handle;
//! - the page trees keep the caller's objects: the rules order copies, and the answer lists the objects themselves;
//! - `_validate_cache_id()` stays WordPress's (functions.php): the ids it refuses never reach the typed use cases,
//!   and a negative id, which it accepts, is dropped there too. No recorded scenario passes one, so it is a
//!   documented difference in the guide, not a `compat/posts-read/breaks.json` entry.
//!
//! `get_post()` dispatches `GetPostObject` (the handler PHP rebinds to change how a post is read by id); the typed
//! `GetPost` handler is for Rust callers, and `get_post()` never reaches it, since it keeps PHP's own objects.
//!
//! An object `$post` is handed over for one dispatch only: every dispatch saves the pending object, puts its own
//! (or none) in place, and restores the saved one after (see [`PhpCurrentPost`]).
//!
//! `WP_Post::get_instance()` keeps its signature and its int checks, but the prepared release swaps its body for the
//! native raw read (`post_instance`), so a direct caller and `WP_Post::filter( 'raw' )` read through the bound post
//! repository and the `posts` cache, as `get_post()` does.
//!
//! Known differences, none of them recorded:
//! - the readers beside `get_post()` (`get_post_field()`, `get_post_status()`, `get_post_type()`,
//!   `clean_post_cache()` and the others) read an object argument as a copy: WordPress's `get_post()` inside them
//!   sanitizes a filterless `stdClass` in place (`filter = 'raw'`, int-cast ids), and here the caller's object is left
//!   as it was;
//! - the objects hooks receive are rebuilt copies: the `get_post_status`, `get_page_uri` and `clean_post_cache`
//!   callbacks get a new `WP_Post`, so a callback that mutates `$post` does not change the caller's object, as it does
//!   in WordPress (a cross-cutting limit of the hook values, as in the meta functions).

use std::rc::Rc;

use speedyspec_container::Container;
use speedyspec_cqrs::{BusError, Command, HandlerRef, Query, QueryRef};
use speedyspec_php::native::NativeError;
use speedyspec_php_ext::{Key, Value};
use speedyspec_php_serialize::semantics::{get, to_php_int, to_php_string};
use speedyspec_php_serialize::{PhpKey, PhpValue};
use speedyspec_wp_posts::{
    CleanPostCache, CountPostsCacheKey, GetAllPageIds, GetAvailablePostMimeTypes, GetEnclosed,
    GetLastPostDate, GetLastPostModified, GetLastPostTime, GetPageByPath, GetPageChildren,
    GetPageHierarchy, GetPageUri, GetPostAncestors, GetPostField, GetPostMimeType, GetPostObject,
    GetPostStatus, GetPostType, GetPung, GetToPing, IsSticky, PageTraverseName, PostEntry,
    PostObject, PrimePostCachesGiven, PrimePostParentIdCaches, SanitizePost, SanitizePostField,
    UpdatePostCacheObjects, UpdatePostCachesObjects, UpdatePostParentCaches, UpdatePostmetaCache,
    WpCacheSetPostsLastChanged, WpCountAttachments, WpCountPosts, WpGetPostParentId,
};
use speedyspec_wp_posts_contracts::{
    LastPostField, MimeFilter, Post, PostError, PostId, PostParent, PostRef, PostTypeArg,
    PostTypeName,
};

use crate::adapters::truthy;
use crate::codec::{copy_php, from_php, helper, is_object, method, php, string, to_php};
use crate::ports::PhpCurrentPost;
use crate::registry::{Native, R, a, hook_error, thrown};

/// The handlers the native functions dispatch through, each resolved once.
pub(crate) struct Handlers {
    object: QueryRef<GetPostObject>,
    ancestors: QueryRef<GetPostAncestors>,
    field: QueryRef<GetPostField>,
    mime_type: QueryRef<GetPostMimeType>,
    status: QueryRef<GetPostStatus>,
    post_type: QueryRef<GetPostType>,
    parent_id: QueryRef<WpGetPostParentId>,
    sanitize: QueryRef<SanitizePost>,
    sanitize_field: QueryRef<SanitizePostField>,
    count_key: QueryRef<CountPostsCacheKey>,
    count_posts: QueryRef<WpCountPosts>,
    count_attachments: QueryRef<WpCountAttachments>,
    all_page_ids: QueryRef<GetAllPageIds>,
    page_by_path: QueryRef<GetPageByPath>,
    page_children: QueryRef<GetPageChildren>,
    page_hierarchy: QueryRef<GetPageHierarchy>,
    page_uri: QueryRef<GetPageUri>,
    last_date: QueryRef<GetLastPostDate>,
    last_modified: QueryRef<GetLastPostModified>,
    last_time: QueryRef<GetLastPostTime>,
    mime_types: QueryRef<GetAvailablePostMimeTypes>,
    update_cache: HandlerRef<UpdatePostCacheObjects>,
    update_caches: HandlerRef<UpdatePostCachesObjects>,
    parent_caches: HandlerRef<UpdatePostParentCaches>,
    clean: HandlerRef<CleanPostCache>,
    meta_cache: HandlerRef<UpdatePostmetaCache>,
    prime_parents: HandlerRef<PrimePostParentIdCaches>,
    last_changed: HandlerRef<WpCacheSetPostsLastChanged>,
    prime_given: HandlerRef<PrimePostCachesGiven>,
    pung: QueryRef<GetPung>,
    to_ping: QueryRef<GetToPing>,
    enclosed: QueryRef<GetEnclosed>,
    sticky: QueryRef<IsSticky>,
    traverse: QueryRef<PageTraverseName>,
}

impl Handlers {
    fn new(c: &Rc<Container>) -> Handlers {
        Handlers {
            object: QueryRef::new(c.clone()),
            ancestors: QueryRef::new(c.clone()),
            field: QueryRef::new(c.clone()),
            mime_type: QueryRef::new(c.clone()),
            status: QueryRef::new(c.clone()),
            post_type: QueryRef::new(c.clone()),
            parent_id: QueryRef::new(c.clone()),
            sanitize: QueryRef::new(c.clone()),
            sanitize_field: QueryRef::new(c.clone()),
            count_key: QueryRef::new(c.clone()),
            count_posts: QueryRef::new(c.clone()),
            count_attachments: QueryRef::new(c.clone()),
            all_page_ids: QueryRef::new(c.clone()),
            page_by_path: QueryRef::new(c.clone()),
            page_children: QueryRef::new(c.clone()),
            page_hierarchy: QueryRef::new(c.clone()),
            page_uri: QueryRef::new(c.clone()),
            last_date: QueryRef::new(c.clone()),
            last_modified: QueryRef::new(c.clone()),
            last_time: QueryRef::new(c.clone()),
            mime_types: QueryRef::new(c.clone()),
            update_cache: HandlerRef::new(c.clone()),
            update_caches: HandlerRef::new(c.clone()),
            parent_caches: HandlerRef::new(c.clone()),
            clean: HandlerRef::new(c.clone()),
            meta_cache: HandlerRef::new(c.clone()),
            prime_parents: HandlerRef::new(c.clone()),
            last_changed: HandlerRef::new(c.clone()),
            prime_given: HandlerRef::new(c.clone()),
            pung: QueryRef::new(c.clone()),
            to_ping: QueryRef::new(c.clone()),
            enclosed: QueryRef::new(c.clone()),
            sticky: QueryRef::new(c.clone()),
            traverse: QueryRef::new(c.clone()),
        }
    }
}

/// What the read natives share for the life of the worker.
pub(crate) struct Reading {
    handlers: Handlers,
    /// The global `$post`, and the object a native function hands over in its place.
    current: Rc<PhpCurrentPost>,
}

impl Reading {
    pub(crate) fn new(container: &Rc<Container>, current: Rc<PhpCurrentPost>) -> Reading {
        Reading {
            handlers: Handlers::new(container),
            current,
        }
    }
}

thread_local! {
    pub(crate) static READING: std::cell::RefCell<Option<Rc<Reading>>> = const { std::cell::RefCell::new(None) };
    /// The object the next dispatch hands over in place of the global (`with_post`).
    static GIVE_NEXT: std::cell::RefCell<Option<PhpValue>> = const { std::cell::RefCell::new(None) };
}

fn reading() -> Result<Rc<Reading>, NativeError> {
    READING
        .with(|r| r.borrow().clone())
        .ok_or_else(|| NativeError::throw("Error", "the posts API is not available"))
}

/// A post read failure as PHP sees it: a callback's exception (or a port's) rethrown, anything else an `Error`.
pub(crate) fn post_error(e: PostError) -> NativeError {
    match e {
        PostError::Hook(hook) => hook_error(hook),
        other => NativeError::throw("Error", other.to_string()),
    }
}

fn bus(e: BusError) -> NativeError {
    NativeError::throw("Error", e.to_string())
}

/// Runs one dispatch with the object `with_post` queued for it (or none) as the pending one, restoring what was
/// pending before, so nothing handed to an outer dispatch reaches a read nested in this one, and nothing this
/// dispatch left untaken outlives it.
fn scoped<T>(r: &Reading, f: impl FnOnce() -> T) -> T {
    let next = GIVE_NEXT.with(|g| g.borrow_mut().take());
    let saved = r.current.replace(next);
    let out = f();
    r.current.replace(saved);
    out
}

fn ask<Q: Query<Result = Result<T, PostError>>, T>(
    pick: impl FnOnce(&Handlers) -> &QueryRef<Q>,
    q: Q,
) -> Result<T, NativeError> {
    let r = reading()?;
    scoped(&r, || pick(&r.handlers).call(q))
        .map_err(bus)?
        .map_err(post_error)
}

fn send<C: Command<Result = Result<T, PostError>>, T>(
    pick: impl FnOnce(&Handlers) -> &HandlerRef<C>,
    c: C,
) -> Result<T, NativeError> {
    let r = reading()?;
    scoped(&r, || pick(&r.handlers).call(c))
        .map_err(bus)?
        .map_err(post_error)
}

fn php_in(value: &Value) -> Result<PhpValue, NativeError> {
    from_php(value).map_err(thrown)
}

fn php_out(value: &PhpValue) -> R {
    to_php(value).map_err(thrown)
}

fn copy(value: &Value) -> Result<PhpValue, NativeError> {
    copy_php(value).map_err(thrown)
}

/// `(string) $value`.
fn bytes(value: &Value) -> Result<Vec<u8>, NativeError> {
    Ok(to_php_string(&php_in(value)?))
}

fn word(value: &Value) -> Result<String, NativeError> {
    Ok(String::from_utf8_lossy(&bytes(value)?).into_owned())
}

/// `(int) $value`.
fn int(value: &Value) -> Result<i64, NativeError> {
    Ok(to_php_int(&php_in(value)?))
}

fn is_numeric(value: &Value) -> Result<bool, NativeError> {
    Ok(truthy(
        &php("is_numeric", std::slice::from_ref(value)).map_err(thrown)?,
    ))
}

fn ints(ids: impl Iterator<Item = i64>) -> Value {
    Value::Array(
        ids.enumerate()
            .map(|(i, id)| (Key::Int(i as i64), Value::Int(id)))
            .collect(),
    )
}

fn pid(id: PostId) -> i64 {
    i64::try_from(id.0).unwrap_or(i64::MAX)
}

/// A value's entries (`(array) $value` of an array); nothing for anything else.
fn entries(value: &Value) -> Vec<(Key, Value)> {
    match value.read().into_owned() {
        Value::Array(items) => items,
        _ => Vec::new(),
    }
}

/// `$post` as WordPress's readers take it.
enum PostArg {
    /// `get_post( $post )` gives null: a non-numeric scalar, an array, an id below 1.
    Missing,
    /// An id, or the current post for an empty argument.
    Ref(PostRef),
    /// An object, read in place of the global.
    Object(PhpValue),
}

fn post_arg(value: &Value) -> Result<PostArg, NativeError> {
    if !truthy(value) {
        return Ok(PostArg::Ref(PostRef::Current));
    }
    if is_object(value) {
        return Ok(PostArg::Object(copy(value)?));
    }
    if is_numeric(value)? {
        let id = int(value)?;
        return Ok(match u64::try_from(id) {
            Ok(id) if id > 0 => PostArg::Ref(PostRef::Id(PostId(id))),
            _ => PostArg::Missing,
        });
    }
    Ok(PostArg::Missing)
}

/// Runs a use case on `$post`: `None` when there is no post to read.
fn with_post<T>(
    value: &Value,
    f: impl FnOnce(PostRef) -> Result<T, NativeError>,
) -> Result<Option<T>, NativeError> {
    match post_arg(value)? {
        PostArg::Missing => Ok(None),
        PostArg::Ref(post) => f(post).map(Some),
        PostArg::Object(object) => {
            GIVE_NEXT.with(|g| *g.borrow_mut() = Some(object));
            let out = f(PostRef::Current);
            GIVE_NEXT.with(|g| g.borrow_mut().take());
            out.map(Some)
        }
    }
}

/// `$post instanceof WP_Post`.
fn is_wp_post(value: &Value) -> Result<bool, NativeError> {
    Ok(
        is_object(value)
            && truthy(&helper("is_post", std::slice::from_ref(value)).map_err(thrown)?),
    )
}

/// `WP_Post::get_instance( $id )` through the use case: `new WP_Post( $entry )`, or `None` (false).
fn instance(id: i64) -> Result<Option<Instance>, NativeError> {
    let Ok(id) = u64::try_from(id) else {
        return Ok(None);
    };
    if id == 0 {
        return Ok(None);
    }
    let found = ask(
        |h| &h.object,
        GetPostObject {
            post: PostRef::Id(PostId(id)),
        },
    )?;
    match found {
        Some(PostObject::Entry(entry)) => Ok(Some(Instance {
            post: crate::codec::post_entry(&entry).map_err(thrown)?,
            raw: true,
        })),
        Some(PostObject::Object(PhpValue::Object { properties, .. })) => {
            let raw = matches!(get(&properties, "filter"), Some(PhpValue::Str(f)) if f == b"raw");
            Ok(Some(Instance {
                post: crate::codec::post_object_owned(properties).map_err(thrown)?,
                raw,
            }))
        }
        _ => Ok(None),
    }
}

/// `WP_Post::get_instance( $post_id )`'s body in the prepared release (`release::GET_INSTANCE_BODY`), after its int
/// checks: the post read by id through the use case, or `false`.
fn post_instance(x: &[Value]) -> R {
    Ok(Instance::post(instance(int(&a(x, 0))?)?).unwrap_or(Value::Bool(false)))
}

/// A `WP_Post` [`instance`] built, and whether its `filter` is the string `raw`.
struct Instance {
    post: Value,
    raw: bool,
}

impl Instance {
    fn post(found: Option<Instance>) -> Option<Value> {
        found.map(|i| i.post)
    }
}

/// `sanitize_post( $object, $context )` on a live object: its properties through the use case, written back to it.
fn sanitize_object(object: &Value, context: &str) -> Result<(), NativeError> {
    let copied = copy(object)?;
    let sanitized = ask(
        |h| &h.sanitize,
        SanitizePost {
            post: copied,
            context: context.to_owned(),
        },
    )?;
    if let PhpValue::Object { properties, .. } = sanitized {
        let properties = php_out(&PhpValue::Array(properties))?;
        helper("assign", &[object.clone(), properties]).map_err(thrown)?;
    }
    Ok(())
}

/// `get_post( $post, $output, $filter )` (post.php 1146-1186), see the module's docs.
///
/// A post read by id is a `WP_Post` built here, whose class is final: when its `filter` is `raw` and `raw` is asked
/// for, `WP_Post::filter()` would return the object itself (`$this->filter === $filter`), so it is not called.
fn get_post_value(post: Value, output: &Value, filter: &Value) -> R {
    let mut post = post;
    if !truthy(&post)
        && let Some(global) = speedyspec_php::global("post")
        && !matches!(&*global.read(), Value::Null)
    {
        post = global;
    }
    let mut built_raw = false;
    let int_id = match &*post.read() {
        Value::Int(id) => Some(*id),
        _ => None,
    };
    let found = if let Some(id) = int_id {
        // is_numeric() of an int is true and (int) is the int itself.
        instance(id)?.map(|i| {
            built_raw = i.raw;
            i.post
        })
    } else if is_wp_post(&post)? {
        Some(post)
    } else if is_object(&post) {
        let properties = match copy(&post)? {
            PhpValue::Object { properties, .. } => properties,
            _ => Vec::new(),
        };
        let own_filter = get(&properties, "filter").cloned();
        if own_filter
            .as_ref()
            .is_none_or(speedyspec_php_serialize::semantics::is_empty)
        {
            sanitize_object(&post, "raw")?;
            Some(helper("post", &[post.clone()]).map_err(thrown)?)
        } else if matches!(&own_filter, Some(PhpValue::Str(f)) if f == b"raw") {
            Some(helper("post", &[post.clone()]).map_err(thrown)?)
        } else {
            match speedyspec_php_serialize::semantics::isset(&properties, "ID") {
                Some(id) => Instance::post(instance(to_php_int(id))?),
                None => None,
            }
        }
    } else if is_numeric(&post)? {
        Instance::post(instance(int(&post)?)?)
    } else {
        None
    };
    let Some(found) = found else {
        return Ok(Value::Null);
    };
    let filtered = if built_raw && matches!(&*filter.read(), Value::Str(f) if f == "raw") {
        found
    } else {
        method(&found, "filter", std::slice::from_ref(filter)).map_err(thrown)?
    };
    if !truthy(&filtered) {
        return Ok(Value::Null);
    }
    let output = output.read();
    match &*output {
        Value::Str(s) if s == "ARRAY_A" => method(&filtered, "to_array", &[]).map_err(thrown),
        Value::Str(s) if s == "ARRAY_N" => {
            let array = method(&filtered, "to_array", &[]).map_err(thrown)?;
            php("array_values", &[array]).map_err(thrown)
        }
        _ => Ok(filtered),
    }
}

fn get_post(x: &[Value]) -> R {
    // The output and filter are only read: borrowed, not copied.
    get_post_value(
        a(x, 0),
        x.get(1).unwrap_or(&Value::Null),
        x.get(2).unwrap_or(&Value::Null),
    )
}

fn get_post_field(x: &[Value]) -> R {
    let field = bytes(&a(x, 0))?;
    let context = word(&a(x, 2))?;
    let found = with_post(&a(x, 1), |post| {
        ask(
            |h| &h.field,
            GetPostField {
                field,
                post,
                context,
            },
        )
    })?;
    match found {
        Some(value) => php_out(&value),
        None => Ok(Value::Str(String::new())),
    }
}

fn get_post_mime_type(x: &[Value]) -> R {
    let found = with_post(&a(x, 0), |post| {
        ask(|h| &h.mime_type, GetPostMimeType { post })
    })?;
    Ok(match found.flatten() {
        Some(mime) => string(mime.as_bytes()),
        None => Value::Bool(false),
    })
}

/// With no `$post`, a global `WP_Post` filtered `sample` (`get_sample_permalink()` sets it on the global on the edit
/// screen) is read again by its `ID`, as `get_post()` does through `WP_Post::filter( 'raw' )`.
fn status_arg(value: Value) -> Result<Value, NativeError> {
    if truthy(&value) {
        return Ok(value);
    }
    let Some(global) = speedyspec_php::global("post") else {
        return Ok(value);
    };
    if !is_wp_post(&global)? {
        return Ok(value);
    }
    match copy(&global)? {
        PhpValue::Object { properties, .. } if matches!(get(&properties, "filter"), Some(PhpValue::Str(f)) if f == b"sample") =>
        {
            let id = get(&properties, "ID").map_or(0, to_php_int);
            // `(int) $ID`: below 1 is no post (a non-numeric argument is one).
            Ok(if id > 0 {
                Value::Int(id)
            } else {
                Value::Str("x".into())
            })
        }
        _ => Ok(value),
    }
}

fn get_post_status(x: &[Value]) -> R {
    let found = with_post(&status_arg(a(x, 0))?, |post| {
        ask(|h| &h.status, GetPostStatus { post })
    })?;
    match found {
        Some(status) => php_out(&status),
        None => Ok(Value::Bool(false)),
    }
}

fn get_post_type(x: &[Value]) -> R {
    let found = with_post(&a(x, 0), |post| ask(|h| &h.post_type, GetPostType { post }))?;
    Ok(match found.flatten() {
        Some(name) => Value::Str(name.as_str().to_owned()),
        None => Value::Bool(false),
    })
}

fn get_post_ancestors(x: &[Value]) -> R {
    let found = with_post(&a(x, 0), |post| {
        ask(|h| &h.ancestors, GetPostAncestors { post })
    })?;
    Ok(ints(found.unwrap_or_default().into_iter().map(pid)))
}

fn wp_get_post_parent_id(x: &[Value]) -> R {
    let found = with_post(&a(x, 0), |post| {
        ask(|h| &h.parent_id, WpGetPostParentId { post })
    })?;
    Ok(match found.flatten() {
        Some(parent) => Value::Int(pid(parent)),
        None => Value::Bool(false),
    })
}

/// `$post_type` as `get_page_by_path()` takes it: an array of types, or one.
fn post_type_arg(value: &Value) -> Result<PostTypeArg, NativeError> {
    Ok(match value.read().into_owned() {
        Value::Array(items) => PostTypeArg::Many(
            items
                .iter()
                .map(|(_, v)| word(v).map(PostTypeName::new))
                .collect::<Result<_, _>>()?,
        ),
        other => PostTypeArg::One(PostTypeName::new(word(&other)?)),
    })
}

fn get_page_by_path(x: &[Value]) -> R {
    let path = bytes(&a(x, 0))?;
    let post_type = post_type_arg(&a(x, 2))?;
    let found = ask(|h| &h.page_by_path, GetPageByPath { path, post_type })?;
    match found {
        // return get_post( $foundid, $output ): the post is cached now, so this reads no row.
        Some(page) => get_post_value(
            Value::Int(pid(page.id)),
            &a(x, 1),
            &Value::Str("raw".into()),
        ),
        None => Ok(Value::Null),
    }
}

/// The posts of a page list as the tree rules read them, each tagged with its place in the list (in `guid`, which
/// the rules never read), so the answer can list the caller's own objects.
fn tagged_pages(pages: &[(Key, Value)]) -> Result<Vec<Post>, NativeError> {
    let mut out = Vec::new();
    for (i, (_, page)) in pages.iter().enumerate() {
        if !is_object(page) {
            continue;
        }
        let properties = match copy(page)? {
            PhpValue::Object { properties, .. } => properties,
            _ => Vec::new(),
        };
        let mut post = PostEntry::from_properties(&properties);
        post.guid = i.to_string().into_bytes();
        out.push(post);
    }
    Ok(out)
}

/// A page id as `isset( $children[ $page_id ] )` compares it with `(int) $page->post_parent`: `None` when no
/// parent can match.
fn page_key(value: &Value) -> Result<Option<PostId>, NativeError> {
    let read = value.read().into_owned();
    let id = match &read {
        Value::Int(i) => Some(*i),
        Value::Bool(b) => Some(i64::from(*b)),
        Value::Float(f) if f.is_finite() => Some(f.trunc() as i64),
        Value::Str(_) | Value::Bytes(_) => {
            let text = bytes(&read)?;
            speedyspec_php_serialize::semantics::is_integer_key(&text)
                .then(|| to_php_int(&PhpValue::Str(text)))
        }
        _ => None,
    };
    Ok(id.and_then(|i| u64::try_from(i).ok()).map(PostId))
}

fn get_page_children(x: &[Value]) -> R {
    let pages = entries(&a(x, 1));
    let Some(page_id) = page_key(&a(x, 0))? else {
        return Ok(Value::Array(Vec::new()));
    };
    let children = ask(
        |h| &h.page_children,
        GetPageChildren {
            page_id,
            pages: tagged_pages(&pages)?,
        },
    )?;
    Ok(Value::Array(
        children
            .iter()
            .enumerate()
            .filter_map(|(i, child)| {
                let at: usize = std::str::from_utf8(&child.guid).ok()?.parse().ok()?;
                Some((Key::Int(i as i64), pages.get(at)?.1.clone()))
            })
            .collect(),
    ))
}

fn get_page_hierarchy(x: &[Value]) -> R {
    let pages = entries(&a(x, 0));
    if !truthy(&a(x, 0)) {
        return Ok(Value::Array(Vec::new()));
    }
    let Some(page_id) = page_key(&a(x, 1))? else {
        return Ok(Value::Array(Vec::new()));
    };
    let names = ask(
        |h| &h.page_hierarchy,
        GetPageHierarchy {
            pages: tagged_pages(&pages)?,
            page_id,
        },
    )?;
    Ok(Value::Array(
        names
            .into_iter()
            .map(|(id, name)| (Key::Int(pid(id)), string(name.as_bytes())))
            .collect(),
    ))
}

fn page_traverse_name(x: &[Value]) -> R {
    let m = PageTraverseName {
        page_id: php_in(&a(x, 0))?,
        children: copy(&a(x, 1))?,
        result: php_in(&a(x, 2))?,
    };
    php_out(&ask(|h| &h.traverse, m)?)
}

fn get_page_uri(x: &[Value]) -> R {
    let found = with_post(&a(x, 0), |post| ask(|h| &h.page_uri, GetPageUri { post }))?;
    match found {
        Some(uri) => php_out(&uri),
        None => Ok(Value::Bool(false)),
    }
}

fn get_all_page_ids(_: &[Value]) -> R {
    let ids = ask(|h| &h.all_page_ids, GetAllPageIds {})?;
    php_out(&PhpValue::Array(
        ids.into_iter()
            .enumerate()
            .map(|(i, v)| (PhpKey::Int(i as i64), v))
            .collect(),
    ))
}

fn wp_count_posts(x: &[Value]) -> R {
    let m = WpCountPosts {
        post_type: word(&a(x, 0))?,
        perm: word(&a(x, 1))?,
    };
    php_out(&ask(|h| &h.count_posts, m)?)
}

fn count_posts_cache_key(x: &[Value]) -> R {
    let m = CountPostsCacheKey {
        post_type: word(&a(x, 0))?,
        perm: word(&a(x, 1))?,
    };
    Ok(Value::Str(ask(|h| &h.count_key, m)?))
}

fn wp_count_attachments(x: &[Value]) -> R {
    let value = a(x, 0);
    let mime_type = match value.read().into_owned() {
        Value::Array(items) => Some(MimeFilter::Many(
            items
                .iter()
                .map(|(_, v)| word(v))
                .collect::<Result<_, _>>()?,
        )),
        other if !truthy(&other) => None,
        other => Some(MimeFilter::One(word(&other)?)),
    };
    php_out(&ask(
        |h| &h.count_attachments,
        WpCountAttachments { mime_type },
    )?)
}

fn get_available_post_mime_types(x: &[Value]) -> R {
    let types = ask(
        |h| &h.mime_types,
        GetAvailablePostMimeTypes {
            post_type: word(&a(x, 0))?,
        },
    )?;
    php_out(&PhpValue::Array(
        types
            .into_iter()
            .enumerate()
            .map(|(i, v)| (PhpKey::Int(i as i64), v))
            .collect(),
    ))
}

fn get_lastpostdate(x: &[Value]) -> R {
    let m = GetLastPostDate {
        timezone: word(&a(x, 0))?,
        post_type: word(&a(x, 1))?,
    };
    php_out(&ask(|h| &h.last_date, m)?)
}

fn get_lastpostmodified(x: &[Value]) -> R {
    let m = GetLastPostModified {
        timezone: word(&a(x, 0))?,
        post_type: word(&a(x, 1))?,
    };
    php_out(&ask(|h| &h.last_modified, m)?)
}

fn get_last_post_time(x: &[Value]) -> R {
    // in_array( $field, array( 'date', 'modified' ), true ): only those strings.
    let field = match a(x, 1).read().into_owned() {
        Value::Str(s) => LastPostField::parse(&s),
        _ => None,
    };
    let Some(field) = field else {
        return Ok(Value::Bool(false));
    };
    let m = GetLastPostTime {
        timezone: word(&a(x, 0))?,
        field,
        post_type: word(&a(x, 2))?,
    };
    php_out(&ask(|h| &h.last_time, m)?)
}

fn sanitize_post(x: &[Value]) -> R {
    let post = a(x, 0);
    let context = word(&a(x, 1))?;
    if is_object(&post) {
        sanitize_object(&post, &context)?;
        return Ok(post);
    }
    if matches!(&*post.read(), Value::Array(_)) {
        let sanitized = ask(
            |h| &h.sanitize,
            SanitizePost {
                post: php_in(&post)?,
                context,
            },
        )?;
        return php_out(&sanitized);
    }
    Ok(post)
}

fn sanitize_post_field(x: &[Value]) -> R {
    let m = SanitizePostField {
        field: bytes(&a(x, 0))?,
        value: php_in(&a(x, 1))?,
        post_id: int(&a(x, 2))?,
        context: word(&a(x, 3))?,
    };
    php_out(&ask(|h| &h.sanitize_field, m)?)
}

/// The objects of a post list, each sanitized `raw` in place unless its `filter` is `raw` already, as
/// `update_post_cache()` does; then copied for the use case.
fn raw_objects(posts: &Value) -> Result<Vec<PhpValue>, NativeError> {
    let mut out = Vec::new();
    for (_, post) in entries(posts) {
        if !is_object(&post) {
            continue;
        }
        let raw = matches!(
            copy(&post)?,
            PhpValue::Object { ref properties, .. }
                if matches!(get(properties, "filter"), Some(PhpValue::Str(f)) if f == b"raw")
        );
        if !raw {
            sanitize_object(&post, "raw")?;
        }
        out.push(copy(&post)?);
    }
    Ok(out)
}

fn update_post_cache(x: &[Value]) -> R {
    let posts = a(x, 0);
    if !truthy(&posts) {
        return Ok(Value::Null);
    }
    let posts = raw_objects(&posts)?;
    send(|h| &h.update_cache, UpdatePostCacheObjects { posts })?;
    Ok(Value::Null)
}

fn update_post_caches(x: &[Value]) -> R {
    let posts = a(x, 0);
    if !truthy(&posts) {
        return Ok(Value::Null);
    }
    let m = UpdatePostCachesObjects {
        posts: raw_objects(&posts)?,
        post_type: php_in(&a(x, 1))?,
        terms: truthy(&a(x, 2)),
        meta: truthy(&a(x, 3)),
    };
    send(|h| &h.update_caches, m)?;
    Ok(Value::Null)
}

fn update_post_parent_caches(x: &[Value]) -> R {
    // wp_list_pluck( $posts, 'post_parent' ): an object's property or an array's entry.
    let mut posts = Vec::new();
    for (_, post) in entries(&a(x, 0)) {
        let parent = match copy(&post)? {
            PhpValue::Object { properties, .. } | PhpValue::Array(properties) => {
                get(&properties, "post_parent").cloned()
            }
            _ => None,
        };
        if let Some(parent) = parent {
            let parent = to_php_int(&parent).unsigned_abs();
            posts.push(Post {
                parent: PostParent::from(PostId(parent)),
                ..Post::default()
            });
        }
    }
    send(|h| &h.parent_caches, UpdatePostParentCaches { posts })?;
    Ok(Value::Null)
}

/// `$post_ids` as `update_meta_cache()` reads them: a list through `intval`, or a string with everything but digits
/// and commas removed, split on commas.
fn meta_ids(value: &Value) -> Result<Vec<PostId>, NativeError> {
    let id = |i: i64| PostId(u64::try_from(i).unwrap_or(0));
    Ok(match value.read().into_owned() {
        Value::Array(items) => items
            .iter()
            .map(|(_, v)| int(v).map(id))
            .collect::<Result<_, _>>()?,
        other => {
            let kept: Vec<u8> = bytes(&other)?
                .into_iter()
                .filter(|b| b.is_ascii_digit() || *b == b',')
                .collect();
            kept.split(|b| *b == b',')
                .map(|part| {
                    id(std::str::from_utf8(part)
                        .ok()
                        .and_then(|s| s.parse().ok())
                        .unwrap_or(0))
                })
                .collect()
        }
    })
}

fn update_postmeta_cache(x: &[Value]) -> R {
    if !truthy(&a(x, 0)) {
        return Ok(Value::Bool(false));
    }
    let ids = meta_ids(&a(x, 0))?;
    match send(|h| &h.meta_cache, UpdatePostmetaCache { ids })? {
        Some(cache) => php_out(&cache),
        None => Ok(Value::Bool(false)),
    }
}

/// `array_filter( $ids, '_validate_cache_id' )` (WordPress's own, with its `_doing_it_wrong()`), then `intval`;
/// a negative id is dropped as well, since the typed use cases cannot carry it. The ids as given come back too.
fn cache_ids(value: &Value) -> Result<(Vec<PostId>, Vec<PhpValue>), NativeError> {
    let mut ids = Vec::new();
    let mut given = Vec::new();
    for (_, id) in entries(value) {
        given.push(php_in(&id)?);
        let valid = truthy(&php("_validate_cache_id", std::slice::from_ref(&id)).map_err(thrown)?);
        if valid && let Ok(id) = u64::try_from(int(&id)?) {
            ids.push(PostId(id));
        }
    }
    Ok((ids, given))
}

fn prime_post_caches(x: &[Value]) -> R {
    let terms = x.get(1).is_none_or(truthy);
    let meta = x.get(2).is_none_or(truthy);
    let (ids, given) = cache_ids(&a(x, 0))?;
    let m = PrimePostCachesGiven {
        ids,
        given,
        terms,
        meta,
    };
    send(|h| &h.prime_given, m)?;
    Ok(Value::Null)
}

fn prime_post_parent_id_caches(x: &[Value]) -> R {
    let (ids, _) = cache_ids(&a(x, 0))?;
    send(|h| &h.prime_parents, PrimePostParentIdCaches { ids })?;
    Ok(Value::Null)
}

fn wp_cache_set_posts_last_changed(_: &[Value]) -> R {
    send(|h| &h.last_changed, WpCacheSetPostsLastChanged {})?;
    Ok(Value::Null)
}

fn get_pung(x: &[Value]) -> R {
    let found = with_post(&a(x, 0), |post| ask(|h| &h.pung, GetPung { post }))?;
    match found {
        Some(pung) => php_out(&pung),
        None => Ok(Value::Bool(false)),
    }
}

fn get_to_ping(x: &[Value]) -> R {
    let found = with_post(&a(x, 0), |post| ask(|h| &h.to_ping, GetToPing { post }))?;
    match found {
        Some(to_ping) => php_out(&to_ping),
        None => Ok(Value::Bool(false)),
    }
}

fn get_enclosed(x: &[Value]) -> R {
    let m = GetEnclosed {
        post_id: php_in(&a(x, 0))?,
    };
    php_out(&ask(|h| &h.enclosed, m)?)
}

fn is_sticky(x: &[Value]) -> R {
    let m = IsSticky {
        post_id: php_in(&a(x, 0))?,
    };
    php_out(&ask(|h| &h.sticky, m)?)
}

/// The functions the facade calls, with WordPress's names.
pub(crate) const ENTRIES: &[(&str, Native)] = &[
    ("get_post", get_post),
    ("post_instance", post_instance),
    ("get_post_field", get_post_field),
    ("get_post_mime_type", get_post_mime_type),
    ("get_post_status", get_post_status),
    ("get_post_type", get_post_type),
    ("get_post_ancestors", get_post_ancestors),
    ("wp_get_post_parent_id", wp_get_post_parent_id),
    ("get_page", get_post),
    ("get_page_by_path", get_page_by_path),
    ("get_page_children", get_page_children),
    ("get_page_hierarchy", get_page_hierarchy),
    ("_page_traverse_name", page_traverse_name),
    ("get_page_uri", get_page_uri),
    ("get_all_page_ids", get_all_page_ids),
    ("wp_count_posts", wp_count_posts),
    ("_count_posts_cache_key", count_posts_cache_key),
    ("wp_count_attachments", wp_count_attachments),
    (
        "get_available_post_mime_types",
        get_available_post_mime_types,
    ),
    ("get_lastpostdate", get_lastpostdate),
    ("get_lastpostmodified", get_lastpostmodified),
    ("_get_last_post_time", get_last_post_time),
    ("update_post_cache", update_post_cache),
    ("clean_post_cache", clean_post_cache),
    ("update_post_caches", update_post_caches),
    ("update_post_parent_caches", update_post_parent_caches),
    ("update_postmeta_cache", update_postmeta_cache),
    ("_prime_post_caches", prime_post_caches),
    ("_prime_post_parent_id_caches", prime_post_parent_id_caches),
    (
        "wp_cache_set_posts_last_changed",
        wp_cache_set_posts_last_changed,
    ),
    ("sanitize_post", sanitize_post),
    ("sanitize_post_field", sanitize_post_field),
    ("get_pung", get_pung),
    ("get_to_ping", get_to_ping),
    ("get_enclosed", get_enclosed),
    ("is_sticky", is_sticky),
];

fn clean_post_cache(x: &[Value]) -> R {
    with_post(&a(x, 0), |post| send(|h| &h.clean, CleanPostCache { post }))?;
    Ok(Value::Null)
}
