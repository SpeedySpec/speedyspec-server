//! The post type and post status names, and the resolved objects WordPress keeps for them.

use std::fmt;

use speedyspec_php_serialize::{PhpKey, PhpValue};

macro_rules! name {
    ($(#[$doc:meta])* $ty:ident) => {
        $(#[$doc])*
        #[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub struct $ty(String);

        impl $ty {
            /// Wraps a name unchecked; the registration rules sanitize it.
            pub fn new(name: impl Into<String>) -> Self {
                Self(name.into())
            }

            /// The name.
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $ty {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }
    };
}

name!(
    /// A post type's name (`post`, `page`, `book`).
    PostTypeName
);
name!(
    /// A post status' name (`publish`, `draft`).
    PostStatusName
);

fn find(properties: &[(PhpKey, PhpValue)], prop: &str) -> Option<usize> {
    properties
        .iter()
        .position(|(k, _)| matches!(k, PhpKey::Str(s) if s == prop.as_bytes()))
}

fn set(properties: &mut Vec<(PhpKey, PhpValue)>, prop: &str, value: PhpValue) {
    match find(properties, prop) {
        Some(i) => properties[i].1 = value,
        None => properties.push((PhpKey::Str(prop.as_bytes().to_vec()), value)),
    }
}

fn remove(properties: &mut Vec<(PhpKey, PhpValue)>, prop: &str) {
    if let Some(i) = find(properties, prop) {
        properties.remove(i);
    }
}

macro_rules! object {
    ($(#[$doc:meta])* $ty:ident, $name:ident, $class:literal) => {
        $(#[$doc])*
        #[derive(Debug, Clone)]
        pub struct $ty {
            name: $name,
            properties: Vec<(PhpKey, PhpValue)>,
        }

        impl $ty {
            /// An object with these properties, in order.
            pub fn new(name: $name, properties: Vec<(PhpKey, PhpValue)>) -> Self {
                Self { name, properties }
            }

            /// The name.
            pub fn name(&self) -> &$name {
                &self.name
            }

            /// The properties, in `get_object_vars()` order.
            pub fn properties(&self) -> &[(PhpKey, PhpValue)] {
                &self.properties
            }

            /// One property.
            pub fn get(&self, prop: &str) -> Option<&PhpValue> {
                find(&self.properties, prop).map(|i| &self.properties[i].1)
            }

            /// Replaces a property in place, or appends it.
            pub fn set(&mut self, prop: &str, value: PhpValue) {
                set(&mut self.properties, prop, value);
            }

            /// Drops a property, as `unset( $object->prop )` does.
            pub fn remove(&mut self, prop: &str) {
                remove(&mut self.properties, prop);
            }

            /// The PHP object.
            pub fn to_php(&self) -> PhpValue {
                PhpValue::Object {
                    class: $class.to_string(),
                    properties: self.properties.clone(),
                }
            }
        }
    };
}

object!(
    /// A `WP_Post_Type` object's `get_object_vars()`: declared properties first, then dynamic ones. `labels`
    /// and `cap` are `stdClass` objects.
    PostType,
    PostTypeName,
    "WP_Post_Type"
);
object!(
    /// A `$wp_post_statuses` entry: a `stdClass` object.
    PostStatusObject,
    PostStatusName,
    "stdClass"
);
