//! The native post writing functions `post-writes.php` (the facade) calls, one per WordPress function (post.php,
//! WordPress 7.2-RC2): `wp_insert_post()`, `wp_update_post()`, `wp_transition_post_status()`, `set_post_type()`, and
//! the default callbacks default-filters.php and `WP_Post_Type::add_hooks()` register by name
//! (`_transition_post_status()`, `_publish_post_hook()`, `_future_post_hook()`,
//! `wp_check_post_hierarchy_for_loops()`). Each dispatches its message through the handler bound in the container
//! (Command/CQRS), so PHP may override one by binding the same key.
//!
//! This adapter does what the use cases leave to the PHP edge:
//!
//! - `$postarr` crosses as a copy (an object with its class and public properties, which `wp_parse_args()` and
//!   `get_object_vars()` read alike);
//! - a post object a callback or caller passes stays the object itself: it is the handle the hooks receive
//!   ([`crate::PhpPostObjects`]), so the hooks share one instance;
//! - a `WP_Error` the use cases answer is built as WordPress builds it: `new WP_Error( $code, __( $message ), $data )`
//!   with `$wpdb->last_error` as the data of the database errors, and a delegated one (`wp_insert_attachment()`'s)
//!   handed back as PHP holds it;
//! - a callback's exception, a port's, or one `wpdb` hid behind its answer reaches PHP as the very object
//!   (`registry::guarded`), and what WordPress wrote before it stays written.

use std::rc::Rc;

use speedyspec_container::Container;
use speedyspec_cqrs::{BusError, Command, HandlerRef};
use speedyspec_php::native::NativeError;
use speedyspec_php_ext::Value;
use speedyspec_php_serialize::semantics::{get, to_php_int, to_php_string};
use speedyspec_php_serialize::{PhpKey, PhpValue};
use speedyspec_wp_posts::{
    CheckPostHierarchyForLoops, FuturePostHook, PostEntry, PostObjects, PublishPostHook,
    SetPostType, TransitionPostStatusDefault, WpInsertPost, WpTransitionPostStatus, WpUpdatePost,
};
use speedyspec_wp_posts_contracts::{
    PostError, PostId, PostObject, PostStatusName, PostWriteError, Translator, WriteResult,
};

use crate::adapters::{PhpTranslator, truthy};
use crate::codec::{copy_php, php, to_php};
use crate::read::post_error;
use crate::registry::{Native, R, a, thrown};
use crate::write_ports::PhpPostObjects;

/// The handlers the write natives dispatch through, each resolved once.
pub(crate) struct Writing {
    insert: HandlerRef<WpInsertPost>,
    update: HandlerRef<WpUpdatePost>,
    transition: HandlerRef<WpTransitionPostStatus>,
    set_type: HandlerRef<SetPostType>,
    default: HandlerRef<TransitionPostStatusDefault>,
    publish: HandlerRef<PublishPostHook>,
    future: HandlerRef<FuturePostHook>,
    loops: HandlerRef<CheckPostHierarchyForLoops>,
}

impl Writing {
    pub(crate) fn new(c: &Rc<Container>) -> Writing {
        Writing {
            insert: HandlerRef::new(c.clone()),
            update: HandlerRef::new(c.clone()),
            transition: HandlerRef::new(c.clone()),
            set_type: HandlerRef::new(c.clone()),
            default: HandlerRef::new(c.clone()),
            publish: HandlerRef::new(c.clone()),
            future: HandlerRef::new(c.clone()),
            loops: HandlerRef::new(c.clone()),
        }
    }
}

thread_local! {
    pub(crate) static WRITING: std::cell::RefCell<Option<Rc<Writing>>> = const { std::cell::RefCell::new(None) };
}

fn writing() -> Result<Rc<Writing>, NativeError> {
    WRITING
        .with(|w| w.borrow().clone())
        .ok_or_else(|| NativeError::throw("Error", "the post write API is not available"))
}

fn bus(e: BusError) -> NativeError {
    NativeError::throw("Error", e.to_string())
}

fn send<C: Command<Result = Result<T, PostError>>, T>(
    pick: impl FnOnce(&Writing) -> &HandlerRef<C>,
    c: C,
) -> Result<T, NativeError> {
    let w = writing()?;
    pick(&w).call(c).map_err(bus)?.map_err(post_error)
}

fn copy(value: &Value) -> Result<PhpValue, NativeError> {
    copy_php(value).map_err(thrown)
}

fn text(value: &Value) -> Result<String, NativeError> {
    Ok(String::from_utf8_lossy(&to_php_string(&copy(value)?)).into_owned())
}

/// `new WP_Error( $code, $message, $data )`.
fn wp_error(code: &str, message: &str, data: Value) -> R {
    let args = [
        Value::Str(code.to_owned()),
        Value::Str(message.to_owned()),
        data,
    ];
    php("SpeedySpec\\WpPosts\\Php\\wp_error", &args).map_err(thrown)
}

/// What `wp_insert_post()` and `wp_update_post()` return: the id, or the `WP_Error` (its message translated with
/// `__()`, as WordPress translates it).
fn write_result(result: WriteResult) -> R {
    let error = match result {
        Ok(id) => return Ok(Value::Int(i64::try_from(id.0).unwrap_or(i64::MAX))),
        Err(error) => error,
    };
    if let PostWriteError::Delegated(value) = &error {
        return to_php(value).map_err(thrown);
    }
    let (Some(code), Some(message)) = (error.code(), error.message()) else {
        return Err(NativeError::throw("Error", error.to_string()));
    };
    let message = PhpTranslator.translate(message, None).map_err(post_error)?;
    let data = match &error {
        PostWriteError::DbInsert { last_error, .. }
        | PostWriteError::DbUpdate { last_error, .. } => Value::Str(last_error.clone()),
        _ => Value::Str(String::new()),
    };
    wp_error(code, &message, data)
}

fn wp_insert_post(x: &[Value]) -> R {
    let m = WpInsertPost {
        postarr: copy(&a(x, 0))?,
        wp_error: truthy(&a(x, 1)),
        fire_after_hooks: x.get(2).is_none_or(truthy),
    };
    write_result(send(|w| &w.insert, m)?)
}

fn wp_update_post(x: &[Value]) -> R {
    let postarr = match x.first() {
        Some(value) => copy(value)?,
        None => PhpValue::Array(Vec::new()),
    };
    let m = WpUpdatePost {
        postarr,
        wp_error: truthy(&a(x, 1)),
        fire_after_hooks: x.get(2).is_none_or(truthy),
    };
    write_result(send(|w| &w.update, m)?)
}

/// `$post` as the object the transition hooks receive: the caller's own value is the handle, its columns read from
/// it now.
fn post_object(post: &Value) -> Result<PostObject, NativeError> {
    let properties = PhpPostObjects
        .read(post)
        .map_err(post_error)?
        .unwrap_or_default();
    Ok(PostObject::with_handle(
        PostEntry::from_properties(&properties),
        post.clone(),
    ))
}

fn wp_transition_post_status(x: &[Value]) -> R {
    let m = WpTransitionPostStatus {
        new: PostStatusName::new(text(&a(x, 0))?),
        old: PostStatusName::new(text(&a(x, 1))?),
        post: post_object(&a(x, 2))?,
    };
    send(|w| &w.transition, m)?;
    Ok(Value::Null)
}

fn set_post_type(x: &[Value]) -> R {
    let post_id = match x.first() {
        Some(value) => to_php_int(&copy(value)?),
        None => 0,
    };
    let post_type = match x.get(1) {
        Some(value) => copy(value)?,
        None => PhpValue::Str(b"post".to_vec()),
    };
    let m = SetPostType { post_id, post_type };
    Ok(match send(|w| &w.set_type, m)? {
        Some(rows) => Value::Int(i64::try_from(rows).unwrap_or(i64::MAX)),
        None => Value::Bool(false),
    })
}

/// An object's properties as the callbacks read it now; none for a value that is not an object.
fn properties(post: &Value) -> Result<Option<Vec<(PhpKey, PhpValue)>>, NativeError> {
    PhpPostObjects.read(post).map_err(post_error)
}

/// `$post->ID` and `$post->post_type`.
fn id_and_type(properties: &[(PhpKey, PhpValue)]) -> (PostId, String) {
    let id = get(properties, "ID").map_or(0, to_php_int);
    let post_type = get(properties, "post_type")
        .map(|t| String::from_utf8_lossy(&to_php_string(t)).into_owned())
        .unwrap_or_default();
    (PostId(u64::try_from(id).unwrap_or(0)), post_type)
}

fn transition_post_status(x: &[Value]) -> R {
    if let Some(post) = properties(&a(x, 2))? {
        let (post_id, post_type) = id_and_type(&post);
        let m = TransitionPostStatusDefault {
            new: text(&a(x, 0))?,
            old: text(&a(x, 1))?,
            post_id,
            post_type,
        };
        send(|w| &w.default, m)?;
    }
    Ok(Value::Null)
}

fn publish_post_hook(x: &[Value]) -> R {
    let post_id = to_php_int(&copy(&a(x, 0))?);
    send(|w| &w.publish, PublishPostHook { post_id })?;
    Ok(Value::Null)
}

fn future_post_hook(x: &[Value]) -> R {
    if let Some(post) = properties(&a(x, 1))? {
        let (post_id, _) = id_and_type(&post);
        let post_date = get(&post, "post_date")
            .map(to_php_string)
            .unwrap_or_default();
        send(|w| &w.future, FuturePostHook { post_id, post_date })?;
    }
    Ok(Value::Null)
}

fn check_post_hierarchy_for_loops(x: &[Value]) -> R {
    let m = CheckPostHierarchyForLoops {
        post_parent: copy(&a(x, 0))?,
        post_id: copy(&a(x, 1))?,
    };
    to_php(&send(|w| &w.loops, m)?).map_err(thrown)
}

/// The functions the facade calls, with WordPress's names.
pub(crate) const ENTRIES: &[(&str, Native)] = &[
    ("set_post_type", set_post_type),
    ("wp_insert_post", wp_insert_post),
    ("wp_update_post", wp_update_post),
    ("wp_transition_post_status", wp_transition_post_status),
    ("_transition_post_status", transition_post_status),
    ("_future_post_hook", future_post_hook),
    ("_publish_post_hook", publish_post_hook),
    (
        "wp_check_post_hierarchy_for_loops",
        check_post_hierarchy_for_loops,
    ),
];
