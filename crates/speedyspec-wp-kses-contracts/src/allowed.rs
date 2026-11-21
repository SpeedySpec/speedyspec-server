use std::collections::HashSet;
use std::rc::Rc;

use std::hash::{BuildHasherDefault, Hasher};

use indexmap::IndexMap;

use crate::{AttrRules, Bytes};

/// A map keyed by tag or attribute name, in PHP's order. Its keys come from an allowlist (configuration, not
/// request input), so it hashes with a fast non-keyed hash rather than SipHash.
pub type ByteMap<V> = IndexMap<Bytes, V, BuildHasherDefault<NameHasher>>;

/// FxHash over 8-byte words: fast for the short ASCII names allowlists hold.
#[derive(Clone, Copy, Debug, Default)]
pub struct NameHasher(u64);

impl Hasher for NameHasher {
    fn write(&mut self, bytes: &[u8]) {
        const K: u64 = 0x517c_c1b7_2722_0a95;
        let (words, rest) = bytes.as_chunks::<8>();
        for w in words {
            self.0 = (self.0.rotate_left(5) ^ u64::from_le_bytes(*w)).wrapping_mul(K);
        }
        for b in rest {
            self.0 = (self.0.rotate_left(5) ^ u64::from(*b)).wrapping_mul(K);
        }
    }

    fn finish(&self) -> u64 {
        self.0
    }
}

/// What an allowlist says about one tag.
#[derive(Clone, Debug, PartialEq)]
pub enum AllowedAttrs {
    /// PHP `true` or any `empty()` value: the tag is kept, with no attributes.
    Empty,
    /// Attribute name (lowercase) to its rules, in PHP's order.
    Attrs(ByteMap<AttrRules>),
}

/// `$allowed_html` as an array: tag name (lowercase) to what it may carry, in PHP's order.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AllowedHtml {
    pub tags: ByteMap<AllowedAttrs>,
}

impl AllowedHtml {
    /// Finds a tag by its lowercase name; callers lowercase, as kses does with `strtolower()`.
    pub fn get(&self, lower_name: &[u8]) -> Option<&AllowedAttrs> {
        self.tags.get(lower_name)
    }
}

/// The URL schemes kses keeps, as given (kses compares them lowercased, except its `http`/`https` fast path).
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
    /// An array from another language: `html` is what kses reads, `source` the caller's own value, handed unchanged
    /// to hooks that receive `$allowed_html` (a PHP array keeps its callables).
    Foreign {
        html: Rc<AllowedHtml>,
        source: crate::Payload,
    },
}
