//! `get_object_subtype()`: the owning seams answer through [`ObjectSubtypes`], and the `get_object_subtype_{type}`
//! filter runs over that answer.
//!
//! [`SubtypesByType`] binds one port per meta type (each seam binds its own, as the spec's *Object subtypes* asks);
//! an unbound type answers `''`, the switch's default. [`BlogSubtypes`] is WordPress 7.2's `'blog'` case.

use std::collections::HashMap;
use std::rc::Rc;

use speedyspec_php_serialize::semantics::to_php_string;
use speedyspec_wp_hooks_contracts::{HookApi, HookName, HookValue};
use speedyspec_wp_meta_contracts::{MetaType, MetadataError, ObjectId, ObjectSubtypes};

use crate::MetaHookNames;
use crate::api::{filter, int, pv};

/// `get_object_subtype( $object_type, $object_id )`: `subtypes` answers the switch, then the
/// `get_object_subtype_{type}` filter runs with `( $object_subtype, $object_id )`, always, even for `''`.
///
/// A filter that returns something other than a string is read as PHP prints it.
pub fn object_subtype(
    hooks: &dyn HookApi,
    subtypes: &dyn ObjectSubtypes,
    meta_type: &MetaType,
    object: ObjectId,
) -> Result<String, MetadataError> {
    let subtype = subtypes.subtype_of(meta_type, object);
    let name = HookName::from(MetaHookNames::new(meta_type).object_subtype().as_str());
    let filtered = filter(hooks, name, HookValue::Str(subtype.clone()), || {
        vec![int(object.0)]
    })?;
    if let HookValue::Str(s) = &filtered {
        return Ok(s.clone());
    }
    let bytes = to_php_string(&pv(&filtered)?);
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

/// One [`ObjectSubtypes`] per meta type; a type nobody bound answers `''`, as `get_object_subtype()`'s switch does
/// for a custom type.
#[derive(Default)]
pub struct SubtypesByType {
    bound: HashMap<String, Rc<dyn ObjectSubtypes>>,
}

impl SubtypesByType {
    pub fn new() -> SubtypesByType {
        SubtypesByType::default()
    }

    /// Binds `subtypes` for `meta_type`, replacing an earlier binding.
    pub fn bind(mut self, meta_type: &str, subtypes: Rc<dyn ObjectSubtypes>) -> SubtypesByType {
        self.bound.insert(meta_type.to_owned(), subtypes);
        self
    }
}

impl ObjectSubtypes for SubtypesByType {
    fn subtype_of(&self, meta_type: &MetaType, object: ObjectId) -> String {
        self.bound
            .get(meta_type.name())
            .map(|s| s.subtype_of(meta_type, object))
            .unwrap_or_default()
    }
}

/// What `get_object_subtype()`'s `'blog'` case asks: `is_multisite()` and `get_site()`.
pub trait Sites {
    fn is_multisite(&self) -> bool;
    /// `get_site( $id )` finds a site (one `wp_blogs` query in WordPress).
    fn site_exists(&self, id: ObjectId) -> bool;
}

/// WordPress 7.2's `'blog'` case: `''` on a single site or for id 0 (no lookup), `'blog'` for a site that exists.
pub struct BlogSubtypes {
    sites: Rc<dyn Sites>,
}

impl BlogSubtypes {
    pub fn new(sites: Rc<dyn Sites>) -> BlogSubtypes {
        BlogSubtypes { sites }
    }
}

impl ObjectSubtypes for BlogSubtypes {
    fn subtype_of(&self, _meta_type: &MetaType, object: ObjectId) -> String {
        if !self.sites.is_multisite() || object.0 == 0 || !self.sites.site_exists(object) {
            return String::new();
        }
        "blog".to_owned()
    }
}
