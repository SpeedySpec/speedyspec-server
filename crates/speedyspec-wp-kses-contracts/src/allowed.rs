use std::collections::HashSet;
use std::rc::Rc;

use indexmap::IndexMap;

use crate::{AttrRules, Bytes};

/// What an allowlist says about one tag.
#[derive(Clone, Debug, PartialEq)]
pub enum AllowedAttrs {
    /// PHP `true` or any `empty()` value: the tag is kept, with no attributes.
    Empty,
    /// Attribute name (lowercase) to its rules, in PHP's order.
    Attrs(IndexMap<Bytes, AttrRules>),
}

/// `$allowed_html` as an array: tag name (lowercase) to what it may carry, in PHP's order.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AllowedHtml {
    pub tags: IndexMap<Bytes, AllowedAttrs>,
}

impl AllowedHtml {
    /// Finds a tag by its lowercase name; callers lowercase, as kses does with `strtolower()`.
    pub fn get(&self, lower_name: &[u8]) -> Option<&AllowedAttrs> {
        self.tags.get(lower_name)
    }
}

/// The URL schemes kses keeps, lowercase, in order.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Protocols(pub Vec<Bytes>);

/// The attribute names whose values are URLs (`wp_kses_uri_attributes()`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct UriAttributes(pub Vec<Bytes>);

/// The named entities kses keeps as they are (`$allowedentitynames`, `$allowedxmlentitynames`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct EntityNames(pub HashSet<Bytes>);

/// Which entity rules `wp_kses_normalize_entities()` applies.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntityContext {
    Html,
    Xml,
}

/// A named allowlist, as `wp_kses_allowed_html()` takes it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Context {
    Post,
    Data,
    Strip,
    Entities,
    UserDescription,
    PreTermDescription,
    PreUserDescription,
    /// Any other string; after the filter it falls to `data`'s tags, as in PHP.
    Named(Bytes),
}

impl Context {
    /// The PHP string for this context.
    pub fn as_bytes(&self) -> &[u8] {
        match self {
            Context::Post => b"post",
            Context::Data => b"data",
            Context::Strip => b"strip",
            Context::Entities => b"entities",
            Context::UserDescription => b"user_description",
            Context::PreTermDescription => b"pre_term_description",
            Context::PreUserDescription => b"pre_user_description",
            Context::Named(name) => name,
        }
    }

    /// Reads a context from its PHP string.
    pub fn from_bytes(name: &[u8]) -> Context {
        match name {
            b"post" => Context::Post,
            b"data" => Context::Data,
            b"strip" => Context::Strip,
            b"entities" => Context::Entities,
            b"user_description" => Context::UserDescription,
            b"pre_term_description" => Context::PreTermDescription,
            b"pre_user_description" => Context::PreUserDescription,
            other => Context::Named(other.into()),
        }
    }
}

/// `$allowed_html` as callers pass it: a context name, resolved per tag, or an array.
#[derive(Clone, Debug, PartialEq)]
pub enum AllowedSpec {
    Context(Context),
    Explicit(Rc<AllowedHtml>),
}
