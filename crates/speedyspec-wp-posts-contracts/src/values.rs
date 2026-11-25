//! The post type and post status names, and the resolved objects WordPress keeps for them.

use std::fmt;

use speedyspec_php_serialize::semantics::{get, set, unset};
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

macro_rules! object {
    ($(#[$doc:meta])* $ty:ident, $name:ident, $class:literal) => {
        $(#[$doc])*
        ///
        /// The name lives twice: the `name` field is the Rust-side name the registries key the object by, and the
        /// `name` property is what PHP code sees (`$object->name`, `wp_list_pluck( $list, 'name' )`). WordPress sets
        /// the property to the registered key, so the two agree. [`Self::set`] keeps them in step: setting the
        /// `name` property to a string also sets the field (lossily, when the bytes are not UTF-8). [`Self::new`]
        /// takes both as given, and [`Self::remove`] leaves the field alone.
        ///
        /// Property names must not be integer-like (`"5"`): lookups match string keys only.
        #[derive(Debug, Clone)]
        pub struct $ty {
            name: $name,
            properties: Vec<(PhpKey, PhpValue)>,
        }

        impl $ty {
            /// An object with these properties, in order. Its `name` property should be the name.
            pub fn new(name: $name, properties: Vec<(PhpKey, PhpValue)>) -> Self {
                Self { name, properties }
            }

            /// The name the registries key the object by. PHP code reads the `name` property instead.
            pub fn name(&self) -> &$name {
                &self.name
            }

            /// The properties, in `get_object_vars()` order.
            pub fn properties(&self) -> &[(PhpKey, PhpValue)] {
                &self.properties
            }

            /// One property.
            pub fn get(&self, prop: &str) -> Option<&PhpValue> {
                get(&self.properties, prop)
            }

            /// Replaces a property in place, or appends it. A string `name` also sets the name field.
            pub fn set(&mut self, prop: &str, value: PhpValue) {
                if let ("name", PhpValue::Str(bytes)) = (prop, &value) {
                    self.name = $name::new(String::from_utf8_lossy(bytes));
                }
                set(&mut self.properties, prop, value);
            }

            /// Drops a property, as `unset( $object->prop )` does. The name field stays.
            pub fn remove(&mut self, prop: &str) {
                unset(&mut self.properties, prop);
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
