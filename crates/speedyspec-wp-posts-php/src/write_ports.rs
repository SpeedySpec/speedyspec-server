//! The post write use cases' outbound ports over PHP (Adapter), and the container ports PHP may rebind.
//!
//! Each adapter calls the WordPress function its port names, that function's own hooks, cache writes and statements
//! included, until the seam behind it is ported. A PHP exception comes back as [`PostError::Hook`] with the exception
//! as its payload (a cron one as the cron contracts' error with the same payload), so the use case stops there and
//! the native function rethrows the very object. A port whose contract cannot fail ([`PhpSlugTruncation`],
//! [`PhpSlugSettings`], [`PhpRuntime::defined`], [`PhpClock::now_gmt`]) parks the exception instead
//! ([`park`]), and the native function rethrows what was parked.
//!
//! The ports to unported seams (users, taxonomy, comments, rewrite, uploads, `WP_Query`, the theme, the column
//! charsets) and the cron functions are container ports ([`Resolved`]): the adapter here (or, for cron, the Rust
//! adapter the runtime passes in) is the default binding, and a PHP object implementing the port's twin
//! (`SpeedySpec\WpPosts\<Port>`, see [`crate::port_twins`]) bound at the same key replaces it for the request. A binding
//! that fails is an error, and nothing falls back.
//!
//! Patterns: Adapter (each port over PHP), Proxy ([`Resolved`], which resolves the binding on every call), Strategy
//! (the binding PHP chooses).

use std::rc::{Rc, Weak};

use speedyspec_container::Container;
use speedyspec_php_ext::{Key, Value};
use speedyspec_php_serialize::semantics::to_php_string;
use speedyspec_php_serialize::{OpaquePhpValue, PhpKey, PhpValue};
use speedyspec_wp_cron_contracts::{CronError, CronResult};
use speedyspec_wp_hooks_contracts::HookValue;
use speedyspec_wp_meta_contracts::{MetaRepositories, MetaRepository, MetaType};
use speedyspec_wp_posts::{
    PostEntry, PostObjects, Runtime, Scheduling, SlugContext, SlugSettings, WriteFormatting,
};
use speedyspec_wp_posts_contracts as c;
use speedyspec_wp_posts_contracts::{
    Charset, Clock, Comments, Post, PostError, PostId, PostObject, PostQuery, PostTypeName,
    Rewrite, SlugTruncation, Theme, Uploads,
};

use crate::adapters::{ask, exists, is_parked, park, truthy};
use crate::codec::{
    Thrown, from_php, helper, hook_in, is_object, object_vars, php, post_object, string,
};
use crate::ports::{PhpTaxonomy, PhpUsers, call, from_answer, into_php, thrown_error};

/// The container key of the users port (`SpeedySpec\WpPosts\Users`).
pub const USERS: &str = "SpeedySpec\\WpPosts\\Users";
/// The container key of the taxonomy port (`SpeedySpec\WpPosts\Taxonomy`), which the reads and the writes share.
pub const TAXONOMY: &str = "SpeedySpec\\WpPosts\\Taxonomy";
/// The container key of the comments port (`SpeedySpec\WpPosts\Comments`).
pub const COMMENTS: &str = "SpeedySpec\\WpPosts\\Comments";
/// The container key of the rewrite port (`SpeedySpec\WpPosts\Rewrite`).
pub const REWRITE: &str = "SpeedySpec\\WpPosts\\Rewrite";
/// The container key of the uploads port (`SpeedySpec\WpPosts\Uploads`).
pub const UPLOADS: &str = "SpeedySpec\\WpPosts\\Uploads";
/// The container key of the `WP_Query` port (`SpeedySpec\WpPosts\PostQuery`).
pub const POST_QUERY: &str = "SpeedySpec\\WpPosts\\PostQuery";
/// The container key of the theme port (`SpeedySpec\WpPosts\Theme`).
pub const THEME: &str = "SpeedySpec\\WpPosts\\Theme";
/// The container key of the column charset port (`SpeedySpec\WpPosts\Charset`).
pub const CHARSET: &str = "SpeedySpec\\WpPosts\\Charset";
/// The container key the meta area binds its tables at (`SpeedySpec\WpMeta\MetaRepositories`), which
/// `wp_delete_post()` reads meta ids through.
pub const META_REPOSITORIES: &str = "SpeedySpec\\WpMeta\\MetaRepositories";

/// The container key of the cron port (`SpeedySpec\WpPosts\Scheduling`).
pub const SCHEDULING: &str = "SpeedySpec\\WpPosts\\Scheduling";

/// A port resolved from the container on every call (Proxy), so a binding PHP made for the request applies; the
/// default binding when the container is gone. The container is held weakly: it holds this adapter's owners.
pub struct Resolved<P: ?Sized + 'static> {
    container: Weak<Container>,
    key: &'static str,
    fallback: Rc<P>,
}

impl<P: ?Sized + 'static> Resolved<P> {
    /// The port bound at `key`, with `fallback` bound there as its default, which `container` resolves on every call.
    pub fn bind(container: &Rc<Container>, key: &'static str, fallback: Rc<P>) -> Resolved<P> {
        let default = fallback.clone();
        container.bind_port::<P>(key, move |_, _| Ok(default.clone()));
        Resolved::new(container, key, fallback)
    }

    /// The port bound at `key`, resolved from `container` on every call, without binding anything.
    pub fn new(container: &Rc<Container>, key: &'static str, fallback: Rc<P>) -> Resolved<P> {
        Resolved {
            container: Rc::downgrade(container),
            key,
            fallback,
        }
    }

    fn port(&self) -> Result<Rc<P>, String> {
        let Some(container) = self.container.upgrade() else {
            return Ok(self.fallback.clone());
        };
        container
            .make_port::<P>(self.key)
            .map_err(|e| format!("the {} binding failed: {e}", self.key))
    }

    fn get(&self) -> Result<Rc<P>, PostError> {
        self.port().map_err(PostError::Db)
    }
}

/// The taxonomy port as the reads see it: the same binding, its three read methods.
impl speedyspec_wp_posts::Taxonomy for Resolved<dyn c::Taxonomy> {
    fn update_object_term_cache(
        &self,
        ids: &[PostId],
        object_types: &[PhpValue],
    ) -> Result<(), PostError> {
        self.get()?.update_object_term_cache(ids, object_types)
    }

    fn clean_object_term_cache(&self, ids: &[PostId], object_type: &str) -> Result<(), PostError> {
        self.get()?.clean_object_term_cache(ids, object_type)
    }

    fn post_term_fields(
        &self,
        post: &PhpValue,
        taxonomy: &str,
        field: &str,
    ) -> Result<PhpValue, PostError> {
        self.get()?.post_term_fields(post, taxonomy, field)
    }
}

impl c::Taxonomy for Resolved<dyn c::Taxonomy> {
    fn update_object_term_cache(
        &self,
        ids: &[PostId],
        object_types: &[PhpValue],
    ) -> Result<(), PostError> {
        self.get()?.update_object_term_cache(ids, object_types)
    }

    fn clean_object_term_cache(&self, ids: &[PostId], object_type: &str) -> Result<(), PostError> {
        self.get()?.clean_object_term_cache(ids, object_type)
    }

    fn post_term_fields(
        &self,
        post: &PhpValue,
        taxonomy: &str,
        field: &str,
    ) -> Result<PhpValue, PostError> {
        self.get()?.post_term_fields(post, taxonomy, field)
    }

    fn set_post_terms(
        &self,
        post: PostId,
        terms: &PhpValue,
        taxonomy: &str,
        append: bool,
    ) -> Result<PhpValue, PostError> {
        self.get()?.set_post_terms(post, terms, taxonomy, append)
    }

    fn object_taxonomies(&self, post_type: &PostTypeName) -> Result<Vec<String>, PostError> {
        self.get()?.object_taxonomies(post_type)
    }

    fn default_terms(&self, post_type: &PostTypeName) -> Result<Vec<String>, PostError> {
        self.get()?.default_terms(post_type)
    }

    fn assign_terms_cap(&self, taxonomy: &str) -> Result<Option<PhpValue>, PostError> {
        self.get()?.assign_terms_cap(taxonomy)
    }

    fn get_object_terms(
        &self,
        post: PostId,
        taxonomy: &str,
        args: &PhpValue,
    ) -> Result<PhpValue, PostError> {
        self.get()?.get_object_terms(post, taxonomy, args)
    }

    fn get_the_terms(&self, post: PostId, taxonomy: &str) -> Result<PhpValue, PostError> {
        self.get()?.get_the_terms(post, taxonomy)
    }

    fn is_object_in_taxonomy(
        &self,
        post_type: &PostTypeName,
        taxonomy: &str,
    ) -> Result<bool, PostError> {
        self.get()?.is_object_in_taxonomy(post_type, taxonomy)
    }

    fn delete_object_term_relationships(
        &self,
        post: PostId,
        taxonomies: &[String],
    ) -> Result<(), PostError> {
        self.get()?
            .delete_object_term_relationships(post, taxonomies)
    }
}

/// The users port as the reads see it: the same binding, `current_user_can()` with no arguments after the
/// capability.
impl speedyspec_wp_posts::Users for Resolved<dyn c::Users> {
    fn is_user_logged_in(&self) -> Result<bool, PostError> {
        self.get()?.is_user_logged_in()
    }

    fn get_current_user_id(&self) -> Result<u64, PostError> {
        self.get()?.get_current_user_id()
    }

    fn current_user_can(&self, capability: &PhpValue) -> Result<bool, PostError> {
        self.get()?.current_user_can(capability, &[])
    }
}

impl c::Users for Resolved<dyn c::Users> {
    fn is_user_logged_in(&self) -> Result<bool, PostError> {
        self.get()?.is_user_logged_in()
    }

    fn get_current_user_id(&self) -> Result<u64, PostError> {
        self.get()?.get_current_user_id()
    }

    fn current_user_can(
        &self,
        capability: &PhpValue,
        args: &[PhpValue],
    ) -> Result<bool, PostError> {
        self.get()?.current_user_can(capability, args)
    }
}

impl Comments for Resolved<dyn Comments> {
    fn default_comment_status(
        &self,
        post_type: &PostTypeName,
        comment_type: &str,
    ) -> Result<PhpValue, PostError> {
        self.get()?.default_comment_status(post_type, comment_type)
    }

    fn trash_post_comments(&self, post: PostId) -> Result<(), PostError> {
        self.get()?.trash_post_comments(post)
    }

    fn untrash_post_comments(&self, post: PostId) -> Result<(), PostError> {
        self.get()?.untrash_post_comments(post)
    }

    fn delete_post_comments(&self, post: PostId) -> Result<(), PostError> {
        self.get()?.delete_post_comments(post)
    }

    fn defer_counting(&self, defer: bool) -> Result<(), PostError> {
        self.get()?.defer_counting(defer)
    }
}

impl Rewrite for Resolved<dyn Rewrite> {
    fn permalink(&self, post: PostId) -> Result<Option<Vec<u8>>, PostError> {
        self.get()?.permalink(post)
    }
}

impl Uploads for Resolved<dyn Uploads> {
    fn insert_attachment(&self, postarr: PhpValue, wp_error: bool) -> Result<PhpValue, PostError> {
        self.get()?.insert_attachment(postarr, wp_error)
    }

    fn delete_attachment(&self, post: PostId, force: bool) -> Result<PhpValue, PostError> {
        self.get()?.delete_attachment(post, force)
    }

    fn update_attached_file(&self, post: PostId, file: &PhpValue) -> Result<(), PostError> {
        self.get()?.update_attached_file(post, file)
    }

    fn attachment_is(&self, kind: &str, post: PostId) -> Result<bool, PostError> {
        self.get()?.attachment_is(kind, post)
    }

    fn set_post_thumbnail(&self, post: PostId, thumbnail: i64) -> Result<(), PostError> {
        self.get()?.set_post_thumbnail(post, thumbnail)
    }

    fn delete_post_thumbnail(&self, post: PostId) -> Result<(), PostError> {
        self.get()?.delete_post_thumbnail(post)
    }
}

impl PostQuery for Resolved<dyn PostQuery> {
    fn query(&self, args: PhpValue) -> Result<PhpValue, PostError> {
        self.get()?.query(args)
    }

    fn get_posts(&self, args: PhpValue) -> Result<PhpValue, PostError> {
        self.get()?.get_posts(args)
    }
}

impl Theme for Resolved<dyn Theme> {
    fn page_templates(&self, post: &Post, page_template: &PhpValue) -> Result<PhpValue, PostError> {
        self.get()?.page_templates(post, page_template)
    }

    fn current_theme_supports(&self, feature: &str, arg: &str) -> Result<bool, PostError> {
        self.get()?.current_theme_supports(feature, arg)
    }
}

impl Charset for Resolved<dyn Charset> {
    fn col_charset(&self, table: &str, column: &str) -> Result<Option<String>, PostError> {
        self.get()?.col_charset(table, column)
    }
}

impl Resolved<dyn Scheduling> {
    fn cron(&self) -> Result<Rc<dyn Scheduling>, CronError> {
        self.port().map_err(CronError::new)
    }
}

impl Scheduling for Resolved<dyn Scheduling> {
    fn clear_scheduled_hook(&self, hook: &str, args: &HookValue) -> Result<CronResult, CronError> {
        self.cron()?.clear_scheduled_hook(hook, args)
    }

    fn schedule_single_event(
        &self,
        timestamp: &HookValue,
        hook: &str,
        args: &HookValue,
    ) -> Result<CronResult, CronError> {
        self.cron()?.schedule_single_event(timestamp, hook, args)
    }

    fn next_scheduled(&self, hook: &str, args: &HookValue) -> Result<HookValue, CronError> {
        self.cron()?.next_scheduled(hook, args)
    }
}

fn int(id: PostId) -> Value {
    Value::Int(i64::try_from(id.0).unwrap_or(i64::MAX))
}

fn word(text: &str) -> Value {
    Value::Str(text.to_owned())
}

/// A PHP list of strings.
fn words(list: &[String]) -> Value {
    Value::Array(
        list.iter()
            .enumerate()
            .map(|(i, w)| (Key::Int(i as i64), word(w)))
            .collect(),
    )
}

/// An answer's values as strings, in order: `(string)` of each entry of an array, nothing for anything else.
fn answer_words(value: &Value) -> Result<Vec<String>, PostError> {
    let PhpValue::Array(items) = from_answer(value)? else {
        return Ok(Vec::new());
    };
    Ok(items
        .iter()
        .map(|(_, v)| String::from_utf8_lossy(&to_php_string(v)).into_owned())
        .collect())
}

/// `get_current_user_id()` and the rest, with `current_user_can()`'s arguments after the capability.
impl c::Users for PhpUsers {
    fn is_user_logged_in(&self) -> Result<bool, PostError> {
        speedyspec_wp_posts::Users::is_user_logged_in(self)
    }

    fn get_current_user_id(&self) -> Result<u64, PostError> {
        speedyspec_wp_posts::Users::get_current_user_id(self)
    }

    fn current_user_can(
        &self,
        capability: &PhpValue,
        args: &[PhpValue],
    ) -> Result<bool, PostError> {
        if !exists("current_user_can") {
            return Ok(false);
        }
        let mut values = vec![into_php(capability)?];
        for arg in args {
            values.push(into_php(arg)?);
        }
        Ok(truthy(&call("current_user_can", &values)?))
    }
}

/// The term functions `wp_insert_post()` and `wp_update_post()` call (taxonomy.php, post.php's term wrappers).
impl c::Taxonomy for PhpTaxonomy {
    fn update_object_term_cache(
        &self,
        ids: &[PostId],
        object_types: &[PhpValue],
    ) -> Result<(), PostError> {
        speedyspec_wp_posts::Taxonomy::update_object_term_cache(self, ids, object_types)
    }

    fn clean_object_term_cache(&self, ids: &[PostId], object_type: &str) -> Result<(), PostError> {
        speedyspec_wp_posts::Taxonomy::clean_object_term_cache(self, ids, object_type)
    }

    fn post_term_fields(
        &self,
        post: &PhpValue,
        taxonomy: &str,
        field: &str,
    ) -> Result<PhpValue, PostError> {
        speedyspec_wp_posts::Taxonomy::post_term_fields(self, post, taxonomy, field)
    }

    fn set_post_terms(
        &self,
        post: PostId,
        terms: &PhpValue,
        taxonomy: &str,
        append: bool,
    ) -> Result<PhpValue, PostError> {
        let args = [
            int(post),
            into_php(terms)?,
            word(taxonomy),
            Value::Bool(append),
        ];
        from_answer(&call("wp_set_post_terms", &args)?)
    }

    fn object_taxonomies(&self, post_type: &PostTypeName) -> Result<Vec<String>, PostError> {
        answer_words(&call("get_object_taxonomies", &[word(post_type.as_str())])?)
    }

    fn default_terms(&self, post_type: &PostTypeName) -> Result<Vec<String>, PostError> {
        let answer = helper("default_terms", &[word(post_type.as_str())]).map_err(thrown_error)?;
        answer_words(&answer)
    }

    fn assign_terms_cap(&self, taxonomy: &str) -> Result<Option<PhpValue>, PostError> {
        let answer = helper("assign_terms_cap", &[word(taxonomy)]).map_err(thrown_error)?;
        Ok(match from_answer(&answer)? {
            PhpValue::Array(mut items) if !items.is_empty() => Some(items.remove(0).1),
            _ => None,
        })
    }

    fn get_object_terms(
        &self,
        post: PostId,
        taxonomy: &str,
        args: &PhpValue,
    ) -> Result<PhpValue, PostError> {
        let args = [int(post), word(taxonomy), into_php(args)?];
        from_answer(&call("wp_get_object_terms", &args)?)
    }

    fn get_the_terms(&self, post: PostId, taxonomy: &str) -> Result<PhpValue, PostError> {
        from_answer(&call("get_the_terms", &[int(post), word(taxonomy)])?)
    }

    fn is_object_in_taxonomy(
        &self,
        post_type: &PostTypeName,
        taxonomy: &str,
    ) -> Result<bool, PostError> {
        let args = [word(post_type.as_str()), word(taxonomy)];
        Ok(truthy(&call("is_object_in_taxonomy", &args)?))
    }

    fn delete_object_term_relationships(
        &self,
        post: PostId,
        taxonomies: &[String],
    ) -> Result<(), PostError> {
        call(
            "wp_delete_object_term_relationships",
            &[int(post), words(taxonomies)],
        )?;
        Ok(())
    }
}

/// `get_default_comment_status()` and the comment functions the post writers call (comment.php, post.php).
pub struct PhpComments;

impl Comments for PhpComments {
    fn default_comment_status(
        &self,
        post_type: &PostTypeName,
        comment_type: &str,
    ) -> Result<PhpValue, PostError> {
        let args = [word(post_type.as_str()), word(comment_type)];
        from_answer(&call("get_default_comment_status", &args)?)
    }

    fn trash_post_comments(&self, post: PostId) -> Result<(), PostError> {
        call("wp_trash_post_comments", &[int(post)])?;
        Ok(())
    }

    fn untrash_post_comments(&self, post: PostId) -> Result<(), PostError> {
        call("wp_untrash_post_comments", &[int(post)])?;
        Ok(())
    }

    fn delete_post_comments(&self, post: PostId) -> Result<(), PostError> {
        helper("delete_post_comments", &[int(post)]).map_err(thrown_error)?;
        Ok(())
    }

    fn defer_counting(&self, defer: bool) -> Result<(), PostError> {
        call("wp_defer_comment_counting", &[Value::Bool(defer)])?;
        Ok(())
    }
}

/// `get_permalink()` (link-template.php).
pub struct PhpRewrite;

impl Rewrite for PhpRewrite {
    fn permalink(&self, post: PostId) -> Result<Option<Vec<u8>>, PostError> {
        Ok(match from_answer(&call("get_permalink", &[int(post)])?)? {
            PhpValue::Bool(false) => None,
            other => Some(to_php_string(&other)),
        })
    }
}

/// The attachment functions the post writers hand over to (post.php, media.php).
pub struct PhpUploads;

impl Uploads for PhpUploads {
    fn insert_attachment(&self, postarr: PhpValue, wp_error: bool) -> Result<PhpValue, PostError> {
        let args = [
            into_php(&postarr)?,
            Value::Bool(false),
            Value::Int(0),
            Value::Bool(wp_error),
        ];
        from_answer(&call("wp_insert_attachment", &args)?)
    }

    fn delete_attachment(&self, post: PostId, force: bool) -> Result<PhpValue, PostError> {
        from_answer(&call(
            "wp_delete_attachment",
            &[int(post), Value::Bool(force)],
        )?)
    }

    fn update_attached_file(&self, post: PostId, file: &PhpValue) -> Result<(), PostError> {
        call("update_attached_file", &[int(post), into_php(file)?])?;
        Ok(())
    }

    fn attachment_is(&self, kind: &str, post: PostId) -> Result<bool, PostError> {
        Ok(truthy(&call("wp_attachment_is", &[word(kind), int(post)])?))
    }

    fn set_post_thumbnail(&self, post: PostId, thumbnail: i64) -> Result<(), PostError> {
        call("set_post_thumbnail", &[int(post), Value::Int(thumbnail)])?;
        Ok(())
    }

    fn delete_post_thumbnail(&self, post: PostId) -> Result<(), PostError> {
        call("delete_post_thumbnail", &[int(post)])?;
        Ok(())
    }
}

/// `WP_Query` and `get_posts()` (class-wp-query.php, post.php).
pub struct PhpPostQuery;

impl PostQuery for PhpPostQuery {
    fn query(&self, args: PhpValue) -> Result<PhpValue, PostError> {
        from_answer(&helper("query", &[into_php(&args)?]).map_err(thrown_error)?)
    }

    fn get_posts(&self, args: PhpValue) -> Result<PhpValue, PostError> {
        from_answer(&call("get_posts", &[into_php(&args)?])?)
    }
}

/// `wp_get_theme()->get_page_templates()` and `current_theme_supports()` (theme.php, class-wp-theme.php).
pub struct PhpTheme;

impl Theme for PhpTheme {
    fn page_templates(&self, post: &Post, page_template: &PhpValue) -> Result<PhpValue, PostError> {
        let object = post_object(&PostEntry::raw_properties(post)).map_err(thrown_error)?;
        let args = [object, into_php(page_template)?];
        from_answer(&helper("page_templates", &args).map_err(thrown_error)?)
    }

    fn current_theme_supports(&self, feature: &str, arg: &str) -> Result<bool, PostError> {
        Ok(truthy(&call(
            "current_theme_supports",
            &[word(feature), word(arg)],
        )?))
    }
}

/// `$wpdb->get_col_charset()` on the table `$wpdb` names (`posts` is `$wpdb->posts`).
pub struct PhpCharset;

impl Charset for PhpCharset {
    fn col_charset(&self, table: &str, column: &str) -> Result<Option<String>, PostError> {
        let answer = helper("col_charset", &[word(table), word(column)]).map_err(thrown_error)?;
        Ok(match from_answer(&answer)? {
            PhpValue::Str(name) => Some(String::from_utf8_lossy(&name).into_owned()),
            _ => None,
        })
    }
}

/// `current_time()`, `get_date_from_gmt()`, `get_gmt_from_date()`, `gmdate()` and `strtotime()`, PHP's own, so the
/// dates follow PHP's clock (and the compat recorder's frozen one) and the site's time zone options.
pub struct PhpClock;

/// A PHP string answer as bytes.
fn bytes(value: &Value) -> Result<Vec<u8>, PostError> {
    Ok(to_php_string(&from_answer(value)?))
}

impl Clock for PhpClock {
    fn now_local(&self) -> Result<String, PostError> {
        let now = bytes(&call("current_time", &[word("mysql")])?)?;
        Ok(String::from_utf8_lossy(&now).into_owned())
    }

    fn now_gmt(&self) -> String {
        match ask("gmdate", &[word("Y-m-d H:i:s")]) {
            Some(Value::Str(now)) => now,
            _ => String::new(),
        }
    }

    fn date_from_gmt(&self, gmt: &[u8]) -> Result<Vec<u8>, PostError> {
        bytes(&call("get_date_from_gmt", &[string(gmt)])?)
    }

    fn gmt_from_date(&self, local: &[u8]) -> Result<Vec<u8>, PostError> {
        bytes(&call("get_gmt_from_date", &[string(local)])?)
    }

    fn timestamp(&self, datetime: &[u8]) -> Option<i64> {
        match ask("strtotime", &[string(datetime)])? {
            Value::Int(t) => Some(t),
            _ => None,
        }
    }
}

/// `wp_truncate_slug()` (formatting.php).
pub struct PhpSlugTruncation;

impl SlugTruncation for PhpSlugTruncation {
    fn truncate(&self, slug: &[u8], length: usize) -> Vec<u8> {
        let length = Value::Int(i64::try_from(length).unwrap_or(i64::MAX));
        match ask("wp_truncate_slug", &[string(slug), length]) {
            Some(answer) => match from_php(&answer) {
                Ok(v) => to_php_string(&v),
                Err(thrown) => {
                    park(thrown);
                    Vec::new()
                }
            },
            None => slug.to_vec(),
        }
    }
}

/// `sanitize_title()` and `wp_encode_emoji()` (formatting.php), with their filters.
pub struct PhpWriteFormatting;

impl WriteFormatting for PhpWriteFormatting {
    fn sanitize_title(
        &self,
        title: &PhpValue,
        fallback: &PhpValue,
        context: &str,
    ) -> Result<PhpValue, PostError> {
        let args = [into_php(title)?, into_php(fallback)?, word(context)];
        from_answer(&call("sanitize_title", &args)?)
    }

    fn encode_emoji(&self, content: &PhpValue) -> Result<PhpValue, PostError> {
        from_answer(&call("wp_encode_emoji", &[into_php(content)?])?)
    }
}

/// The `WP_Post` objects the post hooks receive, live (Adapter): `new WP_Post()` built once and passed by handle, so
/// every hook shares one instance and a callback's change to it is what the next one, and the use case, reads.
pub struct PhpPostObjects;

impl PostObjects for PhpPostObjects {
    fn handle(&self, post: &PostObject) -> Result<HookValue, PostError> {
        if let Some(handle) = &post.handle {
            return Ok(handle.clone());
        }
        let object = post_object(&PostEntry::raw_properties(&post.post)).map_err(thrown_error)?;
        if !post.properties.is_empty() {
            let mut properties = Vec::with_capacity(post.properties.len());
            for (name, value) in &post.properties {
                let value = hook_in(value.clone()).map_err(thrown_error)?;
                properties.push((Key::Str(name.clone()), value));
            }
            helper("assign", &[object.clone(), Value::Array(properties)]).map_err(thrown_error)?;
        }
        Ok(object)
    }

    fn read(&self, handle: &HookValue) -> Result<Option<Vec<(PhpKey, PhpValue)>>, PostError> {
        if let Value::Opaque(any) = handle
            && let Some(carried) = any.downcast_ref::<OpaquePhpValue>()
        {
            return Ok(match &carried.0 {
                PhpValue::Object { properties, .. } => Some(properties.clone()),
                _ => None,
            });
        }
        if !is_object(handle) {
            return Ok(None);
        }
        let vars = object_vars(handle).map_err(thrown_error)?;
        let mut properties = Vec::with_capacity(vars.len());
        for (k, v) in vars {
            let name = match k {
                Key::Int(i) => i.to_string().into_bytes(),
                Key::Str(s) => s.into_bytes(),
            };
            properties.push((PhpKey::Str(name), from_php(&v).map_err(thrown_error)?));
        }
        Ok(Some(properties))
    }

    fn set(&self, handle: &HookValue, name: &str, value: PhpValue) -> Result<HookValue, PostError> {
        if let Value::Opaque(any) = handle
            && let Some(OpaquePhpValue(PhpValue::Object { class, properties })) =
                any.downcast_ref::<OpaquePhpValue>()
        {
            let key = PhpKey::Str(name.as_bytes().to_vec());
            let mut properties = properties.clone();
            match properties.iter_mut().find(|(k, _)| *k == key) {
                Some(slot) => slot.1 = value,
                None => properties.push((key, value)),
            }
            return Ok(Value::Opaque(Rc::new(OpaquePhpValue(PhpValue::Object {
                class: class.clone(),
                properties,
            }))));
        }
        if !is_object(handle) {
            return Ok(handle.clone());
        }
        let value = into_php(&value)?;
        helper(
            "assign",
            &[
                handle.clone(),
                Value::Array(vec![(Key::Str(name.to_owned()), value)]),
            ],
        )
        .map_err(thrown_error)?;
        Ok(handle.clone())
    }
}

/// `wp_delete_post_revision()` and `wp_save_post_revision()` (revision.php), WordPress's until phase 6 ports them.
pub struct PhpRevisions;

impl c::Revisions for PhpRevisions {
    fn save_post_revision(&self, post: PostId) -> Result<(), PostError> {
        call("wp_save_post_revision", &[int(post)])?;
        Ok(())
    }

    fn delete_post_revision(&self, revision: PostId) -> Result<(), PostError> {
        call("wp_delete_post_revision", &[int(revision)])?;
        Ok(())
    }
}

/// The meta tables the meta area binds (`SpeedySpec\WpMeta\MetaRepositories`), resolved on every call; no table
/// when the binding fails or the container is gone.
impl MetaRepositories for Resolved<dyn MetaRepositories> {
    fn for_type(&self, meta_type: &MetaType) -> Option<Rc<dyn MetaRepository>> {
        self.port().ok()?.for_type(meta_type)
    }
}

/// No meta tables: the fallback of the resolved meta repositories when the container is gone.
pub struct NoMetaTables;

impl MetaRepositories for NoMetaTables {
    fn for_type(&self, _: &MetaType) -> Option<Rc<dyn MetaRepository>> {
        None
    }
}

/// `$wp_rewrite->feeds`, `$wp_rewrite->pagination_base` and `get_option( 'permalink_structure' )`, as
/// `wp_unique_post_slug()` reads them.
pub struct PhpSlugSettings;

impl SlugSettings for PhpSlugSettings {
    fn slug_context(&self) -> SlugContext {
        let empty = SlugContext {
            feeds: Vec::new(),
            pagination_base: Vec::new(),
            permalink_structure: Vec::new(),
        };
        let Some(answer) = ask(&format!("{}slug_context", crate::codec::HELPERS), &[]) else {
            return empty;
        };
        let parts = match from_php(&answer) {
            Ok(PhpValue::Array(items)) => items,
            Ok(_) => return empty,
            Err(thrown) => {
                park(thrown);
                return empty;
            }
        };
        let part = |i: usize| parts.get(i).map(|(_, v)| v.clone());
        let feeds = match part(0) {
            Some(PhpValue::Array(items)) => items.iter().map(|(_, v)| to_php_string(v)).collect(),
            _ => Vec::new(),
        };
        SlugContext {
            feeds,
            pagination_base: part(1).map(|v| to_php_string(&v)).unwrap_or_default(),
            permalink_structure: part(2).map(|v| to_php_string(&v)).unwrap_or_default(),
        }
    }
}

/// `defined()` and `_doing_it_wrong()`.
pub struct PhpRuntime;

impl Runtime for PhpRuntime {
    fn defined(&self, name: &str) -> bool {
        !is_parked() && ask("defined", &[word(name)]).is_some_and(|v| truthy(&v))
    }

    fn doing_it_wrong(
        &self,
        function: &str,
        message: &str,
        version: &str,
    ) -> Result<(), PostError> {
        if !exists("_doing_it_wrong") {
            return Ok(());
        }
        call(
            "_doing_it_wrong",
            &[word(function), word(message), word(version)],
        )?;
        Ok(())
    }

    fn constant(&self, name: &str) -> Result<Option<PhpValue>, PostError> {
        if !truthy(&call("defined", &[word(name)])?) {
            return Ok(None);
        }
        Ok(Some(from_answer(&call("constant", &[word(name)])?)?))
    }
}

/// A PHP exception as a cron failure: the exception travels whole as the payload.
fn cron_thrown(thrown: Thrown) -> CronError {
    CronError {
        message: "a PHP cron function failed".into(),
        payload: thrown
            .0
            .map(|exception| Rc::new(exception) as Rc<dyn std::any::Any>),
    }
}

/// The cron functions through PHP (`wp_clear_scheduled_hook()`, `wp_schedule_single_event()`,
/// `wp_next_scheduled()`): WordPress's own, or the cron area's natives over the Rust scheduling core. The runtime
/// binds the Rust adapter directly; this one serves where the cron functions are WordPress's PHP (the compat replay).
pub struct PhpScheduling;

impl PhpScheduling {
    fn call(&self, name: &str, args: &[Value]) -> Result<HookValue, CronError> {
        let answer = php(name, args).map_err(cron_thrown)?;
        crate::codec::hook_out(answer).map_err(cron_thrown)
    }
}

impl Scheduling for PhpScheduling {
    fn clear_scheduled_hook(&self, hook: &str, args: &HookValue) -> Result<CronResult, CronError> {
        let args = hook_in(args.clone()).map_err(cron_thrown)?;
        self.call("wp_clear_scheduled_hook", &[word(hook), args])
            .map(CronResult::Value)
    }

    fn schedule_single_event(
        &self,
        timestamp: &HookValue,
        hook: &str,
        args: &HookValue,
    ) -> Result<CronResult, CronError> {
        let timestamp = hook_in(timestamp.clone()).map_err(cron_thrown)?;
        let args = hook_in(args.clone()).map_err(cron_thrown)?;
        self.call("wp_schedule_single_event", &[timestamp, word(hook), args])
            .map(CronResult::Value)
    }

    fn next_scheduled(&self, hook: &str, args: &HookValue) -> Result<HookValue, CronError> {
        let args = hook_in(args.clone()).map_err(cron_thrown)?;
        self.call("wp_next_scheduled", &[word(hook), args])
    }
}
