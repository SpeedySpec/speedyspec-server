//! The PHP twins of the ports to unported code and of the cron port, and [`PhpBound`], the adapter that makes a PHP
//! implementation of any of them the Rust port.
//!
//! `#[php_port]` declares each interface at the start of every request. A PHP object implementing one, bound in the
//! container at the port's key ([`crate::write_ports`]), replaces the default binding for the request. Values cross
//! as PHP holds them: ids are ints, names strings, a post a `WP_Post`, and "nothing" `null` or `false`. An exception
//! the object throws stops the use case and reaches the caller as the same object.
//!
//! Patterns: Adapter ([`PhpBound`] for every port), Bridge (one adapter type, one twin per port).

use std::rc::Rc;

use speedyspec_php_ext::{Key, PhpObject, PortError, Value, php_port};
use speedyspec_php_serialize::PhpValue;
use speedyspec_php_serialize::semantics::{is_empty, to_php_string};
use speedyspec_wp_cron_contracts::{CronError, CronResult};
use speedyspec_wp_hooks_contracts::HookValue;
use speedyspec_wp_posts::{PostEntry, Scheduling};
use speedyspec_wp_posts_contracts as c;
use speedyspec_wp_posts_contracts::{
    Charset, Comments, Post, PostError, PostId, PostQuery, PostTypeName, Rewrite, Theme, Uploads,
};

use crate::codec::{Thrown, copy_php, hook_in, hook_out, post_object, to_php};
use crate::ports::thrown_error;

/// The users port's twin: `is_user_logged_in()`, `get_current_user_id()` and `current_user_can()` (the
/// capability, then its arguments as a list).
#[php_port(name = "SpeedySpec\\WpPosts\\Users")]
pub trait UsersTwin {
    fn is_user_logged_in(&self) -> Result<Value, PortError>;
    fn get_current_user_id(&self) -> Result<Value, PortError>;
    fn current_user_can(&self, capability: Value, args: Value) -> Result<Value, PortError>;
}

/// The taxonomy port's twin: the term cache functions the post reads call, `WP_Post::__get()`'s term branch, and the
/// term functions the post writers call. Ids cross as ints (a list of them for the cache functions), a post as a
/// `WP_Post`, and `assign_terms_cap()` answers `null` where `get_taxonomy()` is false, else the capability.
#[php_port(name = "SpeedySpec\\WpPosts\\Taxonomy")]
pub trait TaxonomyTwin {
    fn update_object_term_cache(
        &self,
        object_ids: Value,
        object_types: Value,
    ) -> Result<Value, PortError>;
    fn clean_object_term_cache(
        &self,
        object_ids: Value,
        object_type: Value,
    ) -> Result<Value, PortError>;
    fn post_term_fields(
        &self,
        post: Value,
        taxonomy: Value,
        field: Value,
    ) -> Result<Value, PortError>;
    fn set_post_terms(
        &self,
        post_id: Value,
        terms: Value,
        taxonomy: Value,
        append: Value,
    ) -> Result<Value, PortError>;
    fn object_taxonomies(&self, post_type: Value) -> Result<Value, PortError>;
    fn default_terms(&self, post_type: Value) -> Result<Value, PortError>;
    fn assign_terms_cap(&self, taxonomy: Value) -> Result<Value, PortError>;
    fn get_object_terms(
        &self,
        post_id: Value,
        taxonomy: Value,
        args: Value,
    ) -> Result<Value, PortError>;
    fn get_the_terms(&self, post_id: Value, taxonomy: Value) -> Result<Value, PortError>;
    fn is_object_in_taxonomy(&self, post_type: Value, taxonomy: Value) -> Result<Value, PortError>;
    fn delete_object_term_relationships(
        &self,
        post_id: Value,
        taxonomies: Value,
    ) -> Result<Value, PortError>;
}

/// The comments port's twin.
#[php_port(name = "SpeedySpec\\WpPosts\\Comments")]
pub trait CommentsTwin {
    fn default_comment_status(
        &self,
        post_type: Value,
        comment_type: Value,
    ) -> Result<Value, PortError>;
    fn trash_post_comments(&self, post_id: Value) -> Result<Value, PortError>;
    fn untrash_post_comments(&self, post_id: Value) -> Result<Value, PortError>;
    fn delete_post_comments(&self, post_id: Value) -> Result<Value, PortError>;
    fn defer_counting(&self, defer: Value) -> Result<Value, PortError>;
}

/// The rewrite port's twin: `permalink()` answers the URL, or `false`.
#[php_port(name = "SpeedySpec\\WpPosts\\Rewrite")]
pub trait RewriteTwin {
    fn permalink(&self, post_id: Value) -> Result<Value, PortError>;
}

/// The uploads port's twin.
#[php_port(name = "SpeedySpec\\WpPosts\\Uploads")]
pub trait UploadsTwin {
    fn insert_attachment(&self, postarr: Value, wp_error: Value) -> Result<Value, PortError>;
    fn delete_attachment(&self, post_id: Value, force: Value) -> Result<Value, PortError>;
    fn update_attached_file(&self, post_id: Value, file: Value) -> Result<Value, PortError>;
    fn attachment_is(&self, kind: Value, post_id: Value) -> Result<Value, PortError>;
    fn set_post_thumbnail(&self, post_id: Value, thumbnail_id: Value) -> Result<Value, PortError>;
    fn delete_post_thumbnail(&self, post_id: Value) -> Result<Value, PortError>;
}

/// The revisions port's twin: `wp_save_post_revision()` and `wp_delete_post_revision()`, each given the id.
#[php_port(name = "SpeedySpec\\WpPosts\\Revisions")]
pub trait RevisionsTwin {
    fn save_post_revision(&self, post_id: Value) -> Result<Value, PortError>;
    fn delete_post_revision(&self, revision_id: Value) -> Result<Value, PortError>;
}

/// The `WP_Query` port's twin.
#[php_port(name = "SpeedySpec\\WpPosts\\PostQuery")]
pub trait PostQueryTwin {
    fn query(&self, args: Value) -> Result<Value, PortError>;
    fn get_posts(&self, args: Value) -> Result<Value, PortError>;
}

/// The theme port's twin: `page_templates()` takes the `WP_Post` and the `page_template` to set on it.
#[php_port(name = "SpeedySpec\\WpPosts\\Theme")]
pub trait ThemeTwin {
    fn page_templates(&self, post: Value, page_template: Value) -> Result<Value, PortError>;
    fn current_theme_supports(&self, feature: Value, arg: Value) -> Result<Value, PortError>;
}

/// The column charset port's twin: the charset name, or `false`.
#[php_port(name = "SpeedySpec\\WpPosts\\Charset")]
pub trait CharsetTwin {
    fn col_charset(&self, table: Value, column: Value) -> Result<Value, PortError>;
}

/// The cron port's twin: what each cron function returns, as PHP holds it.
#[php_port(name = "SpeedySpec\\WpPosts\\Scheduling")]
pub trait SchedulingTwin {
    fn clear_scheduled_hook(&self, hook: Value, args: Value) -> Result<Value, PortError>;
    fn schedule_single_event(
        &self,
        timestamp: Value,
        hook: Value,
        args: Value,
    ) -> Result<Value, PortError>;
    fn next_scheduled(&self, hook: Value, args: Value) -> Result<Value, PortError>;
}

/// A PHP object implementing one of the twins above, as the Rust port.
pub struct PhpBound(PhpObject);

impl PhpBound {
    pub fn new(object: PhpObject) -> PhpBound {
        PhpBound(object)
    }

    fn raw(&self, method: &str, args: Vec<Value>) -> Result<Value, Thrown> {
        let callable = Value::Array(vec![
            (Key::Int(0), self.0.value().clone()),
            (Key::Int(1), Value::Str(method.to_owned())),
        ]);
        speedyspec_php::call_catching(&callable, &args).map_err(|failure| Thrown(failure.exception))
    }

    fn call(&self, method: &str, args: Vec<Value>) -> Result<PhpValue, PostError> {
        let answer = self.raw(method, args).map_err(thrown_error)?;
        copy_php(&answer).map_err(thrown_error)
    }

    fn yes(&self, method: &str, args: Vec<Value>) -> Result<bool, PostError> {
        Ok(!is_empty(&self.call(method, args)?))
    }

    fn cron(&self, method: &str, args: Vec<Value>) -> Result<HookValue, CronError> {
        let failed = |thrown: Thrown| CronError {
            message: format!("the bound Scheduling's {method}() failed"),
            payload: thrown
                .0
                .map(|exception| Rc::new(exception) as Rc<dyn std::any::Any>),
        };
        let answer = self.raw(method, args).map_err(failed)?;
        hook_out(answer).map_err(failed)
    }
}

fn id(post: PostId) -> Value {
    Value::Int(i64::try_from(post.0).unwrap_or(i64::MAX))
}

fn ids(list: &[PostId]) -> Value {
    Value::Array(
        list.iter()
            .enumerate()
            .map(|(i, p)| (Key::Int(i as i64), id(*p)))
            .collect(),
    )
}

fn word(text: &str) -> Value {
    Value::Str(text.to_owned())
}

fn php(value: &PhpValue) -> Result<Value, PostError> {
    to_php(value).map_err(thrown_error)
}

fn values(list: &[PhpValue]) -> Result<Value, PostError> {
    Ok(Value::Array(
        list.iter()
            .enumerate()
            .map(|(i, v)| Ok((Key::Int(i as i64), php(v)?)))
            .collect::<Result<_, PostError>>()?,
    ))
}

fn words(answer: PhpValue) -> Vec<String> {
    match answer {
        PhpValue::Array(items) => items
            .iter()
            .map(|(_, v)| String::from_utf8_lossy(&to_php_string(v)).into_owned())
            .collect(),
        _ => Vec::new(),
    }
}

impl c::Users for PhpBound {
    fn is_user_logged_in(&self) -> Result<bool, PostError> {
        self.yes("is_user_logged_in", Vec::new())
    }

    fn get_current_user_id(&self) -> Result<u64, PostError> {
        let answer = self.call("get_current_user_id", Vec::new())?;
        Ok(u64::try_from(speedyspec_php_serialize::semantics::to_php_int(&answer)).unwrap_or(0))
    }

    fn current_user_can(
        &self,
        capability: &PhpValue,
        args: &[PhpValue],
    ) -> Result<bool, PostError> {
        self.yes("current_user_can", vec![php(capability)?, values(args)?])
    }
}

impl c::Taxonomy for PhpBound {
    fn update_object_term_cache(
        &self,
        list: &[PostId],
        object_types: &[PhpValue],
    ) -> Result<(), PostError> {
        self.call(
            "update_object_term_cache",
            vec![ids(list), values(object_types)?],
        )?;
        Ok(())
    }

    fn clean_object_term_cache(&self, list: &[PostId], object_type: &str) -> Result<(), PostError> {
        self.call(
            "clean_object_term_cache",
            vec![ids(list), word(object_type)],
        )?;
        Ok(())
    }

    fn post_term_fields(
        &self,
        post: &PhpValue,
        taxonomy: &str,
        field: &str,
    ) -> Result<PhpValue, PostError> {
        self.call(
            "post_term_fields",
            vec![php(post)?, word(taxonomy), word(field)],
        )
    }

    fn set_post_terms(
        &self,
        post: PostId,
        terms: &PhpValue,
        taxonomy: &str,
        append: bool,
    ) -> Result<PhpValue, PostError> {
        self.call(
            "set_post_terms",
            vec![id(post), php(terms)?, word(taxonomy), Value::Bool(append)],
        )
    }

    fn object_taxonomies(&self, post_type: &PostTypeName) -> Result<Vec<String>, PostError> {
        Ok(words(self.call(
            "object_taxonomies",
            vec![word(post_type.as_str())],
        )?))
    }

    fn default_terms(&self, post_type: &PostTypeName) -> Result<Vec<String>, PostError> {
        Ok(words(
            self.call("default_terms", vec![word(post_type.as_str())])?,
        ))
    }

    fn assign_terms_cap(&self, taxonomy: &str) -> Result<Option<PhpValue>, PostError> {
        Ok(match self.call("assign_terms_cap", vec![word(taxonomy)])? {
            PhpValue::Null => None,
            cap => Some(cap),
        })
    }

    fn get_object_terms(
        &self,
        post: PostId,
        taxonomy: &str,
        args: &PhpValue,
    ) -> Result<PhpValue, PostError> {
        self.call(
            "get_object_terms",
            vec![id(post), word(taxonomy), php(args)?],
        )
    }

    fn get_the_terms(&self, post: PostId, taxonomy: &str) -> Result<PhpValue, PostError> {
        self.call("get_the_terms", vec![id(post), word(taxonomy)])
    }

    fn is_object_in_taxonomy(
        &self,
        post_type: &PostTypeName,
        taxonomy: &str,
    ) -> Result<bool, PostError> {
        self.yes(
            "is_object_in_taxonomy",
            vec![word(post_type.as_str()), word(taxonomy)],
        )
    }

    fn delete_object_term_relationships(
        &self,
        post: PostId,
        taxonomies: &[String],
    ) -> Result<(), PostError> {
        let list = Value::Array(
            taxonomies
                .iter()
                .enumerate()
                .map(|(i, t)| (Key::Int(i as i64), word(t)))
                .collect(),
        );
        self.call("delete_object_term_relationships", vec![id(post), list])?;
        Ok(())
    }
}

impl Comments for PhpBound {
    fn default_comment_status(
        &self,
        post_type: &PostTypeName,
        comment_type: &str,
    ) -> Result<PhpValue, PostError> {
        self.call(
            "default_comment_status",
            vec![word(post_type.as_str()), word(comment_type)],
        )
    }

    fn trash_post_comments(&self, post: PostId) -> Result<(), PostError> {
        self.call("trash_post_comments", vec![id(post)])?;
        Ok(())
    }

    fn untrash_post_comments(&self, post: PostId) -> Result<(), PostError> {
        self.call("untrash_post_comments", vec![id(post)])?;
        Ok(())
    }

    fn delete_post_comments(&self, post: PostId) -> Result<(), PostError> {
        self.call("delete_post_comments", vec![id(post)])?;
        Ok(())
    }

    fn defer_counting(&self, defer: bool) -> Result<(), PostError> {
        self.call("defer_counting", vec![Value::Bool(defer)])?;
        Ok(())
    }
}

impl Rewrite for PhpBound {
    fn permalink(&self, post: PostId) -> Result<Option<Vec<u8>>, PostError> {
        Ok(match self.call("permalink", vec![id(post)])? {
            PhpValue::Null | PhpValue::Bool(false) => None,
            url => Some(to_php_string(&url)),
        })
    }
}

impl Uploads for PhpBound {
    fn insert_attachment(&self, postarr: PhpValue, wp_error: bool) -> Result<PhpValue, PostError> {
        self.call(
            "insert_attachment",
            vec![php(&postarr)?, Value::Bool(wp_error)],
        )
    }

    fn delete_attachment(&self, post: PostId, force: bool) -> Result<PhpValue, PostError> {
        self.call("delete_attachment", vec![id(post), Value::Bool(force)])
    }

    fn update_attached_file(&self, post: PostId, file: &PhpValue) -> Result<(), PostError> {
        self.call("update_attached_file", vec![id(post), php(file)?])?;
        Ok(())
    }

    fn attachment_is(&self, kind: &str, post: PostId) -> Result<bool, PostError> {
        self.yes("attachment_is", vec![word(kind), id(post)])
    }

    fn set_post_thumbnail(&self, post: PostId, thumbnail: i64) -> Result<(), PostError> {
        self.call("set_post_thumbnail", vec![id(post), Value::Int(thumbnail)])?;
        Ok(())
    }

    fn delete_post_thumbnail(&self, post: PostId) -> Result<(), PostError> {
        self.call("delete_post_thumbnail", vec![id(post)])?;
        Ok(())
    }
}

impl c::Revisions for PhpBound {
    fn save_post_revision(&self, post: PostId) -> Result<(), PostError> {
        self.call("save_post_revision", vec![id(post)])?;
        Ok(())
    }

    fn delete_post_revision(&self, revision: PostId) -> Result<(), PostError> {
        self.call("delete_post_revision", vec![id(revision)])?;
        Ok(())
    }
}

impl PostQuery for PhpBound {
    fn query(&self, args: PhpValue) -> Result<PhpValue, PostError> {
        self.call("query", vec![php(&args)?])
    }

    fn get_posts(&self, args: PhpValue) -> Result<PhpValue, PostError> {
        self.call("get_posts", vec![php(&args)?])
    }
}

impl Theme for PhpBound {
    fn page_templates(&self, post: &Post, page_template: &PhpValue) -> Result<PhpValue, PostError> {
        let object = post_object(&PostEntry::raw_properties(post)).map_err(thrown_error)?;
        self.call("page_templates", vec![object, php(page_template)?])
    }

    fn current_theme_supports(&self, feature: &str, arg: &str) -> Result<bool, PostError> {
        self.yes("current_theme_supports", vec![word(feature), word(arg)])
    }
}

impl Charset for PhpBound {
    fn col_charset(&self, table: &str, column: &str) -> Result<Option<String>, PostError> {
        Ok(
            match self.call("col_charset", vec![word(table), word(column)])? {
                PhpValue::Str(name) => Some(String::from_utf8_lossy(&name).into_owned()),
                _ => None,
            },
        )
    }
}

impl Scheduling for PhpBound {
    fn clear_scheduled_hook(&self, hook: &str, args: &HookValue) -> Result<CronResult, CronError> {
        let args = hook_in(args.clone()).map_err(|_| CronError::new("cron arguments"))?;
        self.cron("clear_scheduled_hook", vec![word(hook), args])
            .map(CronResult::Value)
    }

    fn schedule_single_event(
        &self,
        timestamp: &HookValue,
        hook: &str,
        args: &HookValue,
    ) -> Result<CronResult, CronError> {
        let timestamp = hook_in(timestamp.clone()).map_err(|_| CronError::new("cron timestamp"))?;
        let args = hook_in(args.clone()).map_err(|_| CronError::new("cron arguments"))?;
        self.cron("schedule_single_event", vec![timestamp, word(hook), args])
            .map(CronResult::Value)
    }

    fn next_scheduled(&self, hook: &str, args: &HookValue) -> Result<HookValue, CronError> {
        let args = hook_in(args.clone()).map_err(|_| CronError::new("cron arguments"))?;
        self.cron("next_scheduled", vec![word(hook), args])
    }
}

/// The twins' adapters for the container ports.
pub(crate) fn users(object: PhpObject) -> Rc<dyn c::Users> {
    Rc::new(PhpBound::new(object))
}

pub(crate) fn taxonomy(object: PhpObject) -> Rc<dyn c::Taxonomy> {
    Rc::new(PhpBound::new(object))
}

pub(crate) fn comments(object: PhpObject) -> Rc<dyn Comments> {
    Rc::new(PhpBound::new(object))
}

pub(crate) fn rewrite(object: PhpObject) -> Rc<dyn Rewrite> {
    Rc::new(PhpBound::new(object))
}

pub(crate) fn uploads(object: PhpObject) -> Rc<dyn Uploads> {
    Rc::new(PhpBound::new(object))
}

pub(crate) fn revisions(object: PhpObject) -> Rc<dyn c::Revisions> {
    Rc::new(PhpBound::new(object))
}

pub(crate) fn post_query(object: PhpObject) -> Rc<dyn PostQuery> {
    Rc::new(PhpBound::new(object))
}

pub(crate) fn theme(object: PhpObject) -> Rc<dyn Theme> {
    Rc::new(PhpBound::new(object))
}

pub(crate) fn charset(object: PhpObject) -> Rc<dyn Charset> {
    Rc::new(PhpBound::new(object))
}

pub(crate) fn scheduling(object: PhpObject) -> Rc<dyn Scheduling> {
    Rc::new(PhpBound::new(object))
}
