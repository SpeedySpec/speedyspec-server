use speedyspec_data::DbError;
use speedyspec_data_counter::{checked_increment, parse_counter};

#[test]
fn counter_rules() {
    assert_eq!(parse_counter(b"0"), Some(0));
    assert_eq!(parse_counter(b"-12"), Some(-12));
    assert_eq!(parse_counter(b"9223372036854775807"), Some(i64::MAX));
    for bad in [b"05".as_slice(), b"-0", b"+1", b"1 ", b""] {
        assert_eq!(
            parse_counter(bad),
            None,
            "{:?}",
            String::from_utf8_lossy(bad)
        );
    }
    assert_eq!(checked_increment("e", None, 5).unwrap(), (5, b"5".to_vec()));
    assert!(
        matches!(checked_increment("e", Some(b"abc"), 1), Err(DbError::Engine { code: Some(c), .. }) if c == "not-integer")
    );
    assert!(
        matches!(checked_increment("e", Some(b"9223372036854775807"), 1), Err(DbError::Engine { code: Some(c), .. }) if c == "overflow")
    );
}
