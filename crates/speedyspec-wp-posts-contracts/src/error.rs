//! What the post type and post APIs report when they cannot do as WordPress does.

use std::fmt;

/// A `WP_Error` the post type functions return, with WordPress 7.2-RC2's code and English message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PostTypeError {
    /// `register_post_type()`: the name is empty or over 20 characters.
    NameLength,
    /// `unregister_post_type()`: the post type is not registered.
    Invalid,
    /// `unregister_post_type()`: the post type is built in.
    Builtin,
}

impl PostTypeError {
    /// The `WP_Error` code.
    pub fn code(&self) -> &'static str {
        match self {
            PostTypeError::NameLength => "post_type_length_invalid",
            PostTypeError::Invalid | PostTypeError::Builtin => "invalid_post_type",
        }
    }

    /// The `WP_Error` message, WordPress's English verbatim.
    pub fn message(&self) -> &'static str {
        match self {
            PostTypeError::NameLength => {
                "Post type names must be between 1 and 20 characters in length."
            }
            PostTypeError::Invalid => "Invalid post type.",
            // No full stop: as in 7.2-RC2.
            PostTypeError::Builtin => "Unregistering a built-in post type is not allowed",
        }
    }
}

impl fmt::Display for PostTypeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.message())
    }
}

impl std::error::Error for PostTypeError {}

/// A post read or write that failed where `$wpdb` would not hide it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PostError {
    /// The database reported a failure.
    Db(String),
}

impl fmt::Display for PostError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PostError::Db(e) => write!(f, "the database failed: {e}"),
        }
    }
}

impl std::error::Error for PostError {}
