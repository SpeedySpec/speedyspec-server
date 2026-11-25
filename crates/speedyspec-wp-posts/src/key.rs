//! `sanitize_key()` and `register_post_type()`'s name check.

use speedyspec_wp_posts_contracts::{PostTypeError, PostTypeName};

/// The version `register_post_type()`'s `_doing_it_wrong()` names for a bad name length.
pub const NAME_LENGTH_VERSION: &str = "4.2.0";

/// `sanitize_key()` without its filter: ASCII-lowercase, then drop every byte outside `[a-z0-9_-]`.
pub fn sanitize_key(raw: &[u8]) -> String {
    raw.iter()
        .map(u8::to_ascii_lowercase)
        .filter(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'_' | b'-'))
        .map(char::from)
        .collect()
}

/// `register_post_type()`'s name rule: the sanitized name, or `NameLength` when it is PHP-`empty()` (`""` or
/// `"0"`) or longer than 20 bytes.
pub fn check_post_type_name(raw: &[u8]) -> Result<PostTypeName, PostTypeError> {
    let name = sanitize_key(raw);
    if name.is_empty() || name == "0" || name.len() > 20 {
        return Err(PostTypeError::NameLength);
    }
    Ok(PostTypeName::new(name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_key_keeps_lowercase_alnum_dash_underscore() {
        assert_eq!(sanitize_key(b"Book Type!"), "booktype");
        assert_eq!(sanitize_key(b"MY_type-2"), "my_type-2");
        assert_eq!(sanitize_key("é".as_bytes()), "");
    }

    #[test]
    fn post_type_name_rule() {
        let twenty_one = "a".repeat(21);
        for bad in ["", "0", "!!!", twenty_one.as_str()] {
            assert_eq!(
                check_post_type_name(bad.as_bytes()),
                Err(PostTypeError::NameLength),
                "{bad:?}"
            );
        }
        let twenty = "a".repeat(20);
        assert_eq!(
            check_post_type_name(twenty.as_bytes()),
            Ok(PostTypeName::new(twenty.clone()))
        );
        assert_eq!(
            check_post_type_name(b"Book Type!"),
            Ok(PostTypeName::new("booktype"))
        );
        assert_eq!(check_post_type_name(b"123"), Ok(PostTypeName::new("123")));
    }
}
