//! `get_post_types()` (post.php, 7.2-RC2 lines 1676-1682), `get_post_stati()` (1567-1573) and
//! `get_post_types_by_support()` (2449-2455): `wp_filter_object_list()` (functions.php 5527-5541) over
//! `WP_List_Util::filter()` (class-wp-list-util.php 100-143), then `pluck( 'name' )` for `names`.
//!
//! The filter is a Specification over each list entry: an argument matches when the entry's field passes its
//! presence test (`isset()` for an object, `array_key_exists()` for an array) and is loosely equal (PHP 8's `==`) to
//! the argument's value. [`ListItem`] is the presence test, one impl per entry shape.
//!
//! The lists return PHP's array keys with each entry, so an adapter can rebuild the `key => value` array exactly:
//! a registry's key is the registered name as PHP converts an array key (`"123"` is the int `123`). `names` plucks
//! each object's `name` property, as `WP_List_Util::pluck( 'name' )` does, not the Rust name field.

use std::borrow::Cow;

use indexmap::IndexSet;
use speedyspec_php_serialize::semantics::{get, loose_eq, to_array_key};
use speedyspec_php_serialize::{PhpKey, PhpValue};
use speedyspec_wp_posts_contracts::{
    PostStatusObject, PostStatuses, PostType, PostTypeSupports, PostTypes,
};

/// `WP_List_Util::filter()`'s operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterOperator {
    /// Every argument matches.
    And,
    /// At least one argument matches.
    Or,
    /// No argument matches.
    Not,
}

impl FilterOperator {
    /// `strtoupper( $operator )` (ASCII, as PHP 8 does) checked against `AND`, `OR` and `NOT`; `None` for anything
    /// else.
    pub fn parse(operator: &str) -> Option<FilterOperator> {
        match operator.to_ascii_uppercase().as_str() {
            "AND" => Some(FilterOperator::And),
            "OR" => Some(FilterOperator::Or),
            "NOT" => Some(FilterOperator::Not),
            _ => None,
        }
    }

    /// Whether an entry matching `matched` of `count` arguments is kept.
    fn keeps(self, matched: usize, count: usize) -> bool {
        match self {
            FilterOperator::And => matched == count,
            FilterOperator::Or => matched > 0,
            FilterOperator::Not => matched == 0,
        }
    }
}

/// One entry of the list `WP_List_Util` filters: `Some` only when the field passes its presence test.
pub trait ListItem {
    /// The field `key` names, when present.
    fn field(&self, key: &PhpKey) -> Option<&PhpValue>;
}

/// A key's bytes: an int key in decimal. PHP names an object property `"5"` for `$obj->{5}`, and Rust keys
/// features by the decimal string, so comparing bytes is PHP's lookup.
fn key_bytes(key: &PhpKey) -> Cow<'_, [u8]> {
    match key {
        PhpKey::Int(i) => Cow::Owned(i.to_string().into_bytes()),
        PhpKey::Str(s) => Cow::Borrowed(s),
    }
}

/// `isset( $obj->{$key} )`: present and not null.
fn isset_property<'a>(properties: &'a [(PhpKey, PhpValue)], key: &PhpKey) -> Option<&'a PhpValue> {
    let wanted = key_bytes(key);
    properties
        .iter()
        .find(|(k, _)| key_bytes(k) == wanted)
        .map(|(_, v)| v)
        .filter(|v| !matches!(v, PhpValue::Null))
}

impl ListItem for PostType {
    /// `isset()`: a null property is absent.
    fn field(&self, key: &PhpKey) -> Option<&PhpValue> {
        isset_property(self.properties(), key)
    }
}

impl ListItem for PostStatusObject {
    /// `isset()`: a null property is absent.
    fn field(&self, key: &PhpKey) -> Option<&PhpValue> {
        isset_property(self.properties(), key)
    }
}

impl ListItem for Vec<(String, PhpValue)> {
    /// A post type's features: `array_key_exists()`, so a null value would still be present.
    fn field(&self, key: &PhpKey) -> Option<&PhpValue> {
        let wanted = key_bytes(key);
        self.iter()
            .find(|(feature, _)| feature.as_bytes() == wanted.as_ref())
            .map(|(_, value)| value)
    }
}

/// `$list[ $name ]`'s key for a registered name: an integer-like name is an int key.
fn registry_key(name: &str) -> PhpKey {
    to_array_key(&PhpValue::Str(name.as_bytes().to_vec())).expect("a string is always a key")
}

/// `pluck( 'name' )` on an object: its `name` property, or null when it has none (PHP warns "Undefined property").
fn pluck_name(properties: &[(PhpKey, PhpValue)]) -> PhpValue {
    get(properties, "name").cloned().unwrap_or(PhpValue::Null)
}

/// `wp_filter_object_list()` without `$field`: empty `args` return every item whatever the operator; an unknown
/// operator returns none; otherwise each item counts its loosely equal matches and is kept for AND (all), OR (any)
/// or NOT (none). Keys are kept, as `WP_List_Util::filter()` keeps them.
pub fn filter_object_list<T: ListItem>(
    items: Vec<(PhpKey, T)>,
    args: &[(PhpKey, PhpValue)],
    operator: &str,
) -> Vec<(PhpKey, T)> {
    if args.is_empty() {
        return items;
    }
    let Some(operator) = FilterOperator::parse(operator) else {
        return Vec::new();
    };
    items
        .into_iter()
        .filter(|(_, item)| {
            let matched = args
                .iter()
                .filter(|(key, wanted)| item.field(key).is_some_and(|got| loose_eq(wanted, got)))
                .count();
            operator.keeps(matched, args.len())
        })
        .collect()
}

/// `get_post_types( $args, 'objects', $operator )`: `key => object` pairs, in registration order.
pub fn post_types(
    types: &dyn PostTypes,
    args: &[(PhpKey, PhpValue)],
    operator: &str,
) -> Vec<(PhpKey, PostType)> {
    let items = types
        .post_types()
        .into_iter()
        .map(|t| (registry_key(t.name().as_str()), t))
        .collect();
    filter_object_list(items, args, operator)
}

/// `get_post_types( $args, 'names', $operator )`: `key => $object->name` pairs.
pub fn post_type_names(
    types: &dyn PostTypes,
    args: &[(PhpKey, PhpValue)],
    operator: &str,
) -> Vec<(PhpKey, PhpValue)> {
    post_types(types, args, operator)
        .into_iter()
        .map(|(key, t)| (key, pluck_name(t.properties())))
        .collect()
}

/// `get_post_stati( $args, 'objects', $operator )`: `key => object` pairs, in registration order.
pub fn post_stati(
    statuses: &dyn PostStatuses,
    args: &[(PhpKey, PhpValue)],
    operator: &str,
) -> Vec<(PhpKey, PostStatusObject)> {
    let items = statuses
        .statuses()
        .into_iter()
        .map(|s| (registry_key(s.name().as_str()), s))
        .collect();
    filter_object_list(items, args, operator)
}

/// `get_post_stati( $args, 'names', $operator )`: `key => $object->name` pairs.
pub fn post_status_names(
    statuses: &dyn PostStatuses,
    args: &[(PhpKey, PhpValue)],
    operator: &str,
) -> Vec<(PhpKey, PhpValue)> {
    post_stati(statuses, args, operator)
        .into_iter()
        .map(|(key, s)| (key, pluck_name(s.properties())))
        .collect()
}

/// `get_post_types_by_support()`: `array_fill_keys( (array) $feature, true )`, so duplicates collapse, filtered
/// over `$_wp_post_type_features`; `array_keys()` of what is left, so the values of a `0..n` list, in that array's
/// order (an integer-like post type is an int).
pub fn post_types_by_support(
    supports: &dyn PostTypeSupports,
    features: &[&str],
    operator: &str,
) -> Vec<PhpKey> {
    let args: Vec<(PhpKey, PhpValue)> = features
        .iter()
        .filter_map(|f| to_array_key(&PhpValue::Str(f.as_bytes().to_vec())))
        .collect::<IndexSet<PhpKey>>()
        .into_iter()
        .map(|key| (key, PhpValue::Bool(true)))
        .collect();
    let items = supports
        .features()
        .into_iter()
        .map(|(post_type, features)| (registry_key(&post_type), features))
        .collect();
    filter_object_list(items, args.as_slice(), operator)
        .into_iter()
        .map(|(post_type, _)| post_type)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use speedyspec_php_serialize::semantics::key_to_string;
    use speedyspec_wp_posts_contracts::{PostStatusName, PostTypeName};

    fn s(v: &str) -> PhpValue {
        PhpValue::Str(v.as_bytes().to_vec())
    }

    fn k(v: &str) -> PhpKey {
        PhpKey::Str(v.as_bytes().to_vec())
    }

    fn post_type(name: &str, props: &[(&str, PhpValue)]) -> PostType {
        PostType::new(
            PostTypeName::new(name),
            std::iter::once((k("name"), s(name)))
                .chain(props.iter().map(|(p, v)| (k(p), v.clone())))
                .collect(),
        )
    }

    struct Types(Vec<PostType>);

    impl PostTypes for Types {
        fn post_type(&self, name: &str) -> Option<PostType> {
            self.0.iter().find(|t| t.name().as_str() == name).cloned()
        }

        fn post_types(&self) -> Vec<PostType> {
            self.0.clone()
        }
    }

    struct Features(Vec<(String, Vec<(String, PhpValue)>)>);

    impl PostTypeSupports for Features {
        fn supports(&self, post_type: &str, feature: &str) -> bool {
            self.0
                .iter()
                .any(|(t, fs)| t == post_type && fs.iter().any(|(f, _)| f == feature))
        }

        fn features(&self) -> Vec<(String, Vec<(String, PhpValue)>)> {
            self.0.clone()
        }
    }

    fn types() -> Types {
        Types(vec![
            post_type("book", &[("public", PhpValue::Bool(true))]),
            post_type(
                "secret",
                &[
                    ("public", PhpValue::Bool(false)),
                    ("menu_position", PhpValue::Null),
                ],
            ),
            post_type("one", &[("public", PhpValue::Int(1))]),
        ])
    }

    /// The plucked names, each checked against its key.
    fn names(found: Vec<(PhpKey, PhpValue)>) -> Vec<String> {
        found
            .into_iter()
            .map(|(key, name)| {
                let PhpValue::Str(name) = name else {
                    panic!("not a string name: {name:?}")
                };
                let name = String::from_utf8(name).unwrap();
                assert_eq!(key_to_string(&key), name);
                name
            })
            .collect()
    }

    fn keys(found: Vec<PhpKey>) -> Vec<String> {
        found.iter().map(key_to_string).collect()
    }

    fn features() -> Features {
        let feature = |f: &str| (f.to_owned(), PhpValue::Bool(true));
        Features(vec![
            ("book".to_owned(), vec![feature("a"), feature("c")]),
            ("guide".to_owned(), vec![feature("a")]),
            ("empty".to_owned(), Vec::new()),
        ])
    }

    #[test]
    fn empty_args_ignore_the_operator() {
        for operator in ["and", "not", "bogus", ""] {
            assert_eq!(
                names(post_type_names(&types(), &[], operator)),
                ["book", "secret", "one"],
                "{operator}"
            );
        }
    }

    #[test]
    fn unknown_operator_is_empty() {
        let args = [(k("public"), PhpValue::Bool(true))];
        assert!(post_type_names(&types(), &args, "xor").is_empty());
        assert!(post_types_by_support(&features(), &["a"], "bogus").is_empty());
    }

    #[test]
    fn null_property_never_matches() {
        // `isset()` fails on null, so even `== null` cannot match; NOT keeps everything.
        let args = [(k("menu_position"), PhpValue::Null)];
        assert!(post_type_names(&types(), &args, "and").is_empty());
        assert_eq!(
            names(post_type_names(&types(), &args, "not")),
            ["book", "secret", "one"]
        );
        // A missing property is absent too.
        let args = [(k("nothing"), PhpValue::Bool(false))];
        assert!(post_type_names(&types(), &args, "and").is_empty());
    }

    #[test]
    fn loose_true_matches_one_and_yes() {
        let types = Types(vec![
            post_type("book", &[("public", PhpValue::Bool(true))]),
            post_type("one", &[("public", PhpValue::Int(1))]),
            post_type("yes", &[("public", s("yes"))]),
            post_type("secret", &[("public", PhpValue::Bool(false))]),
        ]);
        let query =
            |value: PhpValue| names(post_type_names(&types, &[(k("public"), value)], "and"));
        assert_eq!(query(PhpValue::Bool(true)), ["book", "one", "yes"]);
        // `1 == 'yes'` is false in PHP 8: 'yes' is not numeric, so 1 compares as "1".
        assert_eq!(query(PhpValue::Int(1)), ["book", "one"]);
        assert_eq!(query(s("1")), ["book", "one"]);
        // `true == 'yes'` is true, `1 == 'yes'` is false (PHP 8.5.12, the static PHP).
        assert_eq!(query(s("yes")), ["book", "yes"]);
        assert_eq!(query(PhpValue::Bool(false)), ["secret"]);
    }

    #[test]
    fn operator_is_case_insensitive() {
        assert_eq!(FilterOperator::parse("Or"), Some(FilterOperator::Or));
        assert_eq!(FilterOperator::parse("nOT"), Some(FilterOperator::Not));
        assert_eq!(FilterOperator::parse("and"), Some(FilterOperator::And));
        assert_eq!(FilterOperator::parse("xor"), None);
        let args = [
            (k("public"), PhpValue::Bool(true)),
            (k("name"), s("secret")),
        ];
        assert_eq!(
            names(post_type_names(&types(), &args, "oR")),
            ["book", "secret", "one"]
        );
        assert!(post_type_names(&types(), &args, "AND").is_empty());
    }

    #[test]
    fn by_support_dedupes_features() {
        // `array( 'a', 'a' )` is one argument, so AND needs only `a`.
        assert_eq!(
            keys(post_types_by_support(&features(), &["a", "a"], "and")),
            ["book", "guide"]
        );
        assert_eq!(
            keys(post_types_by_support(&features(), &["a", "c"], "and")),
            ["book"]
        );
    }

    #[test]
    fn not_includes_types_with_no_features_left() {
        // A post type whose features were all removed keeps its (empty) entry, and NOT keeps it.
        assert_eq!(
            keys(post_types_by_support(&features(), &["a"], "not")),
            ["empty"]
        );
    }

    #[test]
    fn status_lists_filter_by_isset() {
        struct Statuses(Vec<PostStatusObject>);
        impl PostStatuses for Statuses {
            fn status(&self, name: &str) -> Option<PostStatusObject> {
                self.0.iter().find(|s| s.name().as_str() == name).cloned()
            }
            fn statuses(&self) -> Vec<PostStatusObject> {
                self.0.clone()
            }
        }
        let status = |name: &str, public: PhpValue| {
            PostStatusObject::new(
                PostStatusName::new(name),
                vec![(k("name"), s(name)), (k("public"), public)],
            )
        };
        let statuses = Statuses(vec![
            status("open", PhpValue::Bool(true)),
            status("held", PhpValue::Null),
        ]);
        let args = [(k("public"), PhpValue::Bool(true))];
        assert_eq!(names(post_status_names(&statuses, &args, "and")), ["open"]);
        assert_eq!(post_stati(&statuses, &args, "not").len(), 1);
    }

    #[test]
    fn names_pluck_the_name_property_under_php_keys() {
        // The property is what PHP plucks, even when it differs from the field; a missing one is null.
        let mut renamed = post_type("book", &[]);
        renamed.remove("name");
        renamed.set("name", PhpValue::Int(9));
        let mut nameless = post_type("tome", &[]);
        nameless.remove("name");
        let types = Types(vec![renamed, nameless, post_type("123", &[])]);
        assert_eq!(
            post_type_names(&types, &[], "and"),
            vec![
                (k("book"), PhpValue::Int(9)),
                (k("tome"), PhpValue::Null),
                (PhpKey::Int(123), s("123")),
            ]
        );
        let found: Vec<PhpKey> = post_types(&types, &[], "and")
            .into_iter()
            .map(|(key, _)| key)
            .collect();
        assert_eq!(found, [k("book"), k("tome"), PhpKey::Int(123)]);

        let features = Features(vec![(
            "123".to_owned(),
            vec![("a".to_owned(), PhpValue::Bool(true))],
        )]);
        assert_eq!(
            post_types_by_support(&features, &["a"], "and"),
            [PhpKey::Int(123)]
        );
    }
}
