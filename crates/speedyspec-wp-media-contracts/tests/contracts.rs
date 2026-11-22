use std::time::Duration;

use speedyspec_wp_media_contracts::{Limit, MEDIA_READER, ProbeLimits, ReadError, Tree};

#[test]
fn default_limits_match_the_spec() {
    let limits = ProbeLimits::default();
    assert_eq!(limits.max_bytes, 64 << 20);
    assert_eq!(limits.deadline, Duration::from_secs(5));
    assert_eq!(limits.max_alloc, 256 << 20);
    assert_eq!(limits.max_streams, 32);
    assert_eq!(limits.max_picture_bytes, 16 << 20);
    assert_eq!(limits.max_tag_bytes, 1 << 20);
    assert_eq!(limits.max_memory, 1 << 30);
}

#[test]
fn tree_get_finds_the_first_key() {
    let tree = Tree::Map(vec![("a".into(), Tree::Int(1)), ("a".into(), Tree::Int(2))]);
    assert_eq!(tree.get("a"), Some(&Tree::Int(1)));
    assert_eq!(tree.get("b"), None);
    assert_eq!(Tree::List(vec![]).get("a"), None);
}

#[test]
fn media_reader_key_is_the_php_interface() {
    assert_eq!(MEDIA_READER, "SpeedySpec\\WpMedia\\MediaReader");
}

#[test]
fn read_error_displays_the_limit() {
    assert!(
        ReadError::LimitExceeded(Limit::Bytes)
            .to_string()
            .contains("bytes")
    );
    assert!(ReadError::TimedOut.to_string().contains("time"));
}

#[test]
fn tree_bytes_reads_both_string_kinds() {
    assert_eq!(Tree::Bytes(b"Caf\xe9".to_vec()).bytes(), Some(&b"Caf\xe9"[..]));
    assert_eq!(Tree::Str("x".into()).bytes(), Some(&b"x"[..]));
    assert_eq!(Tree::Int(1).bytes(), None);
}

struct Upper;

impl speedyspec_wp_media_contracts::TagSanitizer for Upper {
    fn post(&self, value: &[u8]) -> Vec<u8> {
        value.to_ascii_uppercase()
    }
    fn post_deep(&self, value: &Tree) -> Tree {
        value.clone()
    }
}

#[test]
fn ports_are_object_safe() {
    let sanitizer: &dyn speedyspec_wp_media_contracts::TagSanitizer = &Upper;
    assert_eq!(sanitizer.post(b"a"), b"A");
    struct Never;
    impl speedyspec_wp_media_contracts::TimeParser for Never {
        fn strtotime(&self, _: &[u8]) -> Option<i64> {
            None
        }
    }
    let time: &dyn speedyspec_wp_media_contracts::TimeParser = &Never;
    assert_eq!(time.strtotime(b"now"), None);
}
