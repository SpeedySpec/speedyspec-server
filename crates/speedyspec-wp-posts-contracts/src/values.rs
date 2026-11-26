//! The post type and post status names, the resolved objects WordPress keeps for them, and the values a post's
//! columns hold.

use std::borrow::Cow;
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
name!(
    /// A post's `comment_status` column: any string it holds (`open`, `closed`).
    CommentStatus
);
name!(
    /// A post's `ping_status` column: any string it holds (`open`, `closed`).
    PingStatus
);

impl CommentStatus {
    /// Whether the status is exactly `open`, as `comments_open()` compares it.
    pub fn is_open(&self) -> bool {
        self.0 == "open"
    }
}

impl PingStatus {
    /// Whether the status is exactly `open`, as `pings_open()` compares it.
    pub fn is_open(&self) -> bool {
        self.0 == "open"
    }
}

/// A post's ID. `0` is "no post": a new post, or `post_parent` for a top-level post.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PostId(pub u64);

/// A `datetime` column as WordPress keeps it (`Y-m-d H:i:s`), unchecked: the date rules validate it.
///
/// It holds a `Cow` so that [`Self::ZERO`] can be a constant; it otherwise behaves as an owned string.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PostDateTime(Cow<'static, str>);

impl PostDateTime {
    /// `0000-00-00 00:00:00`: the columns' default, and WordPress's "no date".
    pub const ZERO: Self = Self(Cow::Borrowed("0000-00-00 00:00:00"));

    /// Wraps a datetime unchecked.
    pub fn new(datetime: impl Into<String>) -> Self {
        Self(Cow::Owned(datetime.into()))
    }

    /// The datetime.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Whether it is exactly [`Self::ZERO`].
    pub fn is_zero(&self) -> bool {
        self.0 == Self::ZERO.0
    }
}

impl Default for PostDateTime {
    fn default() -> Self {
        Self::ZERO
    }
}

impl fmt::Display for PostDateTime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A local and GMT datetime pair: `post_date`/`post_date_gmt`, or `post_modified`/`post_modified_gmt`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct PostDate {
    /// The site's local time.
    pub local: PostDateTime,
    /// The same moment in GMT, or [`PostDateTime::ZERO`] when not yet set (a draft).
    pub gmt: PostDateTime,
}

macro_rules! bytes {
    ($(#[$doc:meta])* $ty:ident) => {
        $(#[$doc])*
        #[derive(Debug, Clone, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub struct $ty(Vec<u8>);

        impl $ty {
            /// Wraps the bytes unchecked.
            pub fn new(bytes: impl Into<Vec<u8>>) -> Self {
                Self(bytes.into())
            }

            /// The bytes.
            pub fn as_bytes(&self) -> &[u8] {
                &self.0
            }
        }

        impl From<&str> for $ty {
            fn from(s: &str) -> Self {
                Self(s.as_bytes().to_vec())
            }
        }
    };
}

bytes!(
    /// A post's `post_name`: its slug, as bytes (PHP strings are bytes; a slug may be percent-encoded or not).
    Slug
);
bytes!(
    /// A post's `post_mime_type` (`image/png`; empty for most post types).
    MimeType
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
