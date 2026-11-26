//! A callback's identity, as WordPress builds it.

/// The shape of a callable, as `_wp_filter_build_unique_id()` sees it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CallbackShape {
    /// A string: a function name or `"Class::method"`.
    Name(String),
    /// A closure or invokable object, by its object id.
    Object(u64),
    /// `[$object, 'method']`.
    Method { object: u64, method: String },
    /// `['Class', 'method']`.
    Static { class: String, method: String },
    /// Anything else.
    Invalid,
}

/// WordPress's `_wp_filter_build_unique_id()`.
pub struct CallbackIdentity;

impl CallbackIdentity {
    pub fn unique_id(shape: &CallbackShape) -> Option<String> {
        match shape {
            CallbackShape::Name(name) => Some(name.clone()),
            CallbackShape::Object(id) => Some(format!("spl_object_id:{id}")),
            CallbackShape::Method { object, method } => Some(format!("{object}{method}")),
            CallbackShape::Static { class, method } => Some(format!("{class}::{method}")),
            CallbackShape::Invalid => None,
        }
    }
}
