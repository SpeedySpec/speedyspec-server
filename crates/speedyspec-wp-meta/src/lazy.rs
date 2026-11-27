//! `WP_Metadata_Lazyloader`: queue objects, then fill the meta cache for all of them on the first read.
//!
//! The queue and the loading are Rust; adding and removing the `get_{type}_metadata` filter callback is PHP's job
//! (the callback is a PHP callable), so [`LazySettings`] says what to hook. Patterns: Observer (the
//! `metadata_lazyloader_queued_objects` action over the [`HookApi`] port), Strategy (the per-type
//! [`LazySettings`]). The loader keeps the pending ids per type as an ordered set keyed by id.

use std::cell::RefCell;
use std::fmt;
use std::rc::Rc;

use indexmap::{IndexMap, IndexSet};
use speedyspec_php_serialize::PhpValue;
use speedyspec_wp_hooks_contracts::{Count, HookApi, HookError, HookName, HookValue, Key};
use speedyspec_wp_meta_contracts::{MetaType, Metadata, MetadataError, ObjectId};

/// What `WP_Metadata_Lazyloader::$settings` holds for one object type.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LazySettings {
    /// The filter to hook while objects are queued: `get_{type}_metadata`.
    pub filter: &'static str,
    /// The callback to hook there, at priority 10 with 5 arguments: `lazyload_meta_callback`.
    pub callback: &'static str,
    /// The deprecated per-type method WordPress keeps (`lazyload_term_meta`, `lazyload_comment_meta`), if any.
    pub deprecated_method: Option<&'static str>,
}

/// WordPress's `$settings`: term, comment and blog.
const SETTINGS: [(&str, LazySettings); 3] = [
    (
        "term",
        LazySettings {
            filter: "get_term_metadata",
            callback: "lazyload_meta_callback",
            deprecated_method: Some("lazyload_term_meta"),
        },
    ),
    (
        "comment",
        LazySettings {
            filter: "get_comment_metadata",
            callback: "lazyload_meta_callback",
            deprecated_method: Some("lazyload_comment_meta"),
        },
    ),
    (
        "blog",
        LazySettings {
            filter: "get_blog_metadata",
            callback: "lazyload_meta_callback",
            deprecated_method: None,
        },
    ),
];

/// A lazy-loader failure.
#[derive(Debug)]
pub enum LazyError {
    /// The type has no lazy-load settings: WordPress's `invalid_object_type` `WP_Error`.
    UnknownType(String),
    Hook(HookError),
    Metadata(MetadataError),
}

impl LazyError {
    /// The `WP_Error` code WordPress gives the failure.
    pub fn code(&self) -> &'static str {
        match self {
            LazyError::UnknownType(_) => "invalid_object_type",
            _ => "",
        }
    }

    /// The hook failure behind this error, from the queued-objects action or from loading the meta cache.
    pub fn hook_error(&self) -> Option<&HookError> {
        match self {
            LazyError::Hook(e) | LazyError::Metadata(MetadataError::Hook(e)) => Some(e),
            _ => None,
        }
    }

    /// What a language bridge rethrows: the payload of the hook failure (a PHP exception object), if any.
    pub fn payload(&self) -> Option<&Rc<dyn std::any::Any>> {
        self.hook_error().and_then(|e| e.payload.as_ref())
    }
}

impl fmt::Display for LazyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LazyError::UnknownType(message) => f.write_str(message),
            LazyError::Hook(e) => write!(f, "a hook failed: {e}"),
            LazyError::Metadata(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for LazyError {}

impl From<HookError> for LazyError {
    fn from(e: HookError) -> Self {
        LazyError::Hook(e)
    }
}

impl From<MetadataError> for LazyError {
    fn from(e: MetadataError) -> Self {
        LazyError::Metadata(e)
    }
}

/// `__( 'Invalid object type.' )`, the message of WordPress's `WP_Error`.
pub const INVALID_OBJECT_TYPE: &str = "Invalid object type.";

/// The lazy loader. One per request, as WordPress keeps one in `wp_metadata_lazyloader()`.
pub struct LazyLoader {
    hooks: Rc<dyn HookApi>,
    metadata: Rc<dyn Metadata>,
    /// `$pending_objects`: per type, the queued ids in first-queued order.
    pending: RefCell<IndexMap<String, IndexSet<u64>>>,
}

impl LazyLoader {
    pub fn new(hooks: Rc<dyn HookApi>, metadata: Rc<dyn Metadata>) -> LazyLoader {
        LazyLoader {
            hooks,
            metadata,
            pending: RefCell::new(IndexMap::new()),
        }
    }

    /// The settings of `object_type`: what PHP hooks while objects are queued, `None` for an unknown type.
    pub fn settings(object_type: &str) -> Option<LazySettings> {
        SETTINGS
            .iter()
            .find(|(name, _)| *name == object_type)
            .map(|(_, s)| *s)
    }

    /// The types that can be queued.
    pub fn types() -> impl Iterator<Item = &'static str> {
        SETTINGS.iter().map(|(name, _)| *name)
    }

    /// `queue_objects()` with the loader object left out of the action's arguments (`null`).
    pub fn queue(&self, object_type: &str, ids: &[ObjectId]) -> Result<(), LazyError> {
        self.queue_with(object_type, ids, HookValue::Null)
    }

    /// `queue_objects()`: remembers the ids, then fires `metadata_lazyloader_queued_objects` with `$object_ids`,
    /// `$object_type` and `$this`; `loader` is that third argument (PHP passes its own object).
    ///
    /// The caller hooks [`LazySettings::callback`] on [`LazySettings::filter`] (10, 5) before the action fires,
    /// as WordPress does.
    pub fn queue_with(
        &self,
        object_type: &str,
        ids: &[ObjectId],
        loader: HookValue,
    ) -> Result<(), LazyError> {
        Self::known(object_type)?;
        {
            let mut pending = self.pending.borrow_mut();
            let set = pending.entry(object_type.to_owned()).or_default();
            for id in ids {
                set.insert(id.0);
            }
        }
        let name = HookName::from("metadata_lazyloader_queued_objects");
        if self.hooks.skip_if_empty(&name, Count::Actions)? {
            return Ok(());
        }
        let list = HookValue::Array(
            ids.iter()
                .enumerate()
                .map(|(i, id)| (Key::Int(i as i64), HookValue::Int(id.0 as i64)))
                .collect(),
        );
        self.hooks.do_action(
            &name,
            vec![list, HookValue::Str(object_type.into()), loader],
        )?;
        Ok(())
    }

    /// `reset_queue()`: forgets the queued ids of the type. The caller removes the filter callback.
    pub fn reset(&self, object_type: &str) -> Result<(), LazyError> {
        Self::known(object_type)?;
        self.pending
            .borrow_mut()
            .insert(object_type.to_owned(), IndexSet::new());
        Ok(())
    }

    /// The ids queued for `object_type`, in the order they were queued.
    pub fn pending(&self, object_type: &str) -> Vec<ObjectId> {
        self.pending
            .borrow()
            .get(object_type)
            .map(|set| set.iter().map(|id| ObjectId(*id)).collect())
            .unwrap_or_default()
    }

    /// `lazyload_meta_callback()` for the deprecated per-type wrappers: no object id.
    pub fn load_for(&self, meta_type: &MetaType, check: PhpValue) -> Result<PhpValue, LazyError> {
        self.load_for_object(meta_type, check, None)
    }

    /// `lazyload_meta_callback()`: loads every queued id of the type through `Metadata::update_cache` (plus
    /// `object` when it is not queued), resets the queue and returns `check` unchanged. A type with nothing
    /// queued returns `check` at once. The caller removes the filter callback after this returns.
    pub fn load_for_object(
        &self,
        meta_type: &MetaType,
        check: PhpValue,
        object: Option<ObjectId>,
    ) -> Result<PhpValue, LazyError> {
        let mut ids = self.pending(meta_type.name());
        if ids.is_empty() {
            return Ok(check);
        }
        // `$object_id` of 0 is falsy: nothing to add.
        if let Some(object) = object.filter(|o| o.0 != 0)
            && !ids.contains(&object)
        {
            ids.push(object);
        }
        self.metadata.update_cache(meta_type, &ids)?;
        // No need to run again for this set of objects.
        self.reset(meta_type.name())?;
        Ok(check)
    }

    fn known(object_type: &str) -> Result<(), LazyError> {
        match Self::settings(object_type) {
            Some(_) => Ok(()),
            None => Err(LazyError::UnknownType(INVALID_OBJECT_TYPE.to_owned())),
        }
    }
}
