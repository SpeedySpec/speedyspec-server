use crate::Leaf;

/// An insertion-ordered map, as PHP arrays are ordered.
pub type OrderedMap<K, V> = indexmap::IndexMap<K, V>;

/// A PHP string: bytes, which may not be valid UTF-8.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct PhpString(pub Vec<u8>);

impl PhpString {
    /// The bytes.
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

impl From<&str> for PhpString {
    fn from(s: &str) -> Self {
        PhpString(s.as_bytes().to_vec())
    }
}

/// A CSS value as theme.json writes it: a string, a number, or a reference to another style value.
#[derive(Clone, Debug, PartialEq)]
pub enum CssValue {
    /// A string value.
    Str(PhpString),
    /// An integer value.
    Int(i64),
    /// A float value.
    Float(f64),
    /// `{ "ref": "styles.elements.link.color.text" }`.
    Ref(PhpString),
}

/// A style value: a CSS value, or an object of them (border sides, spacing sides, background objects).
#[derive(Clone, Debug, PartialEq)]
pub enum StyleValue {
    /// One CSS value.
    Css(CssValue),
    /// An object of style values, keyed by name, in the data's order.
    Map(OrderedMap<String, Leaf<StyleValue>>),
}
