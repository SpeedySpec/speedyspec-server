use crate::model::{OrderedMap, PhpString};
use crate::{ByOrigin, Key, Leaf, Value};

/// One preset (a palette colour, a font size, a gradient, …).
///
/// `slug` and `name` are typed because the engine reads them everywhere; every other key (the value key such as
/// `color` or `size`, `fluid`, `fontFace`, …) is kept in `extra` in the data's order.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Preset {
    /// The preset's slug.
    pub slug: Option<Leaf<PhpString>>,
    /// The preset's human-readable name.
    pub name: Option<Leaf<PhpString>>,
    /// Every other key.
    pub extra: OrderedMap<Key, Value>,
}

/// A preset list as it appears in the data.
#[derive(Clone, Debug, PartialEq)]
pub enum Presets {
    /// Keyed by origin, as WordPress keeps presets once a document is constructed.
    ByOrigin(ByOrigin<Leaf<Vec<Leaf<Preset>>>>),
    /// A plain list, as a theme writes it, before construction keys it.
    Unkeyed(Vec<Leaf<Preset>>),
}

/// `settings.spacing.spacingScale` as it appears in the data.
///
/// Only the spacing-size computation reads a scale's fields, and it checks each field itself as WordPress does,
/// so the scale's content stays a PHP value.
#[derive(Clone, Debug, PartialEq)]
pub enum Scales {
    /// Keyed by origin, as WordPress keeps the scale once a document is constructed.
    ByOrigin(ByOrigin<Value>),
    /// The scale as a theme writes it.
    Unkeyed(Value),
}
