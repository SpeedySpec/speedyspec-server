use crate::Value;

/// A theme.json value that either has the shape its schema expects or is kept exactly as given.
///
/// WordPress's sanitizer checks keys, never values, so a theme may put a string where a boolean belongs and
/// WordPress keeps it. A [`Leaf::Raw`] goes back to PHP unchanged.
#[derive(Clone, Debug, PartialEq)]
pub enum Leaf<T> {
    /// The value has the expected shape.
    Typed(T),
    /// Anything else, kept as given.
    Raw(Value),
}

impl<T> Leaf<T> {
    /// The typed value, when the value had the expected shape.
    pub fn typed(&self) -> Option<&T> {
        match self {
            Leaf::Typed(t) => Some(t),
            Leaf::Raw(_) => None,
        }
    }
}
