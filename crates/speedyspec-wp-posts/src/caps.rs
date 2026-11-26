//! Post type capabilities: `get_post_type_capabilities()` and `_post_type_meta_capabilities()` (post.php,
//! 7.2-RC2 lines 2050-2121), without the `$post_type_meta_caps` global, which the registry keeps.

use speedyspec_php_serialize::semantics::{
    array_merge, get, isset, key_to_string, set, to_array_key, to_php_string,
};
use speedyspec_php_serialize::{PhpKey, PhpValue};

/// The meta capabilities `_post_type_meta_capabilities()` remembers, in PHP's `in_array()` list order.
const META_CAPS: [&str; 3] = ["read_post", "delete_post", "edit_post"];

/// `get_post_type_capabilities()`: keys in its order: edit_post, read_post, delete_post, edit_posts,
/// edit_others_posts, delete_posts, publish_posts, read_private_posts; then, when `map_meta_cap`, read,
/// delete_private_posts, delete_published_posts, delete_others_posts, edit_private_posts, edit_published_posts;
/// then array_merge with `capabilities`; then create_posts = edit_posts unless isset.
///
/// A string (or other scalar) `capability_type` gives the bases `(string) $type` and `$type . 's'`; an array
/// gives its `[0]` and `[1]`, `""` where one is missing (PHP warns and concatenates null). A non-array
/// `capabilities` counts as empty (PHP throws a `TypeError` in `array_merge()`).
pub fn post_type_capabilities(
    capability_type: &PhpValue,
    map_meta_cap: bool,
    capabilities: &PhpValue,
) -> Vec<(PhpKey, PhpValue)> {
    let (singular, plural) = match capability_type {
        PhpValue::Array(items) => (base(items, 0), base(items, 1)),
        other => {
            let singular = to_php_string(other);
            let mut plural = singular.clone();
            plural.push(b's');
            (singular, plural)
        }
    };
    let cap = |prefix: &str, base: &[u8]| {
        let mut out = prefix.as_bytes().to_vec();
        out.extend_from_slice(base);
        PhpValue::Str(out)
    };

    let mut defaults = vec![
        ("edit_post", cap("edit_", &singular)),
        ("read_post", cap("read_", &singular)),
        ("delete_post", cap("delete_", &singular)),
        ("edit_posts", cap("edit_", &plural)),
        ("edit_others_posts", cap("edit_others_", &plural)),
        ("delete_posts", cap("delete_", &plural)),
        ("publish_posts", cap("publish_", &plural)),
        ("read_private_posts", cap("read_private_", &plural)),
    ];
    if map_meta_cap {
        defaults.extend([
            ("read", cap("read", b"")),
            ("delete_private_posts", cap("delete_private_", &plural)),
            ("delete_published_posts", cap("delete_published_", &plural)),
            ("delete_others_posts", cap("delete_others_", &plural)),
            ("edit_private_posts", cap("edit_private_", &plural)),
            ("edit_published_posts", cap("edit_published_", &plural)),
        ]);
    }
    let defaults = defaults
        .into_iter()
        .map(|(key, value)| (PhpKey::Str(key.as_bytes().to_vec()), value))
        .collect();

    let given = match capabilities {
        PhpValue::Array(items) => items.clone(),
        _ => Vec::new(),
    };
    let mut caps = array_merge(defaults, given);
    if isset(&caps, "create_posts").is_none() {
        let edit_posts = get(&caps, "edit_posts").cloned().unwrap_or(PhpValue::Null);
        set(&mut caps, "create_posts", edit_posts);
    }
    caps
}

/// `list( $singular_base, $plural_base ) = $capability_type`: one element as a string, `""` when missing.
fn base(items: &[(PhpKey, PhpValue)], index: i64) -> Vec<u8> {
    items
        .iter()
        .find(|(key, _)| *key == PhpKey::Int(index))
        .map_or_else(Vec::new, |(_, value)| to_php_string(value))
}

/// `_post_type_meta_capabilities()`: `(custom, core)` for core in read_post, delete_post, edit_post, in the caps'
/// order. As in `$post_type_meta_caps[ $custom ] = $core`, a custom capability named twice keeps its first
/// position and its last core capability, and the custom capability becomes an array key as PHP converts one
/// (`false` is `"0"`, `true` `"1"`, `1.7` `"1"`), written as text.
pub fn meta_capabilities(caps: &[(PhpKey, PhpValue)]) -> Vec<(String, &'static str)> {
    let mut out: Vec<(String, &'static str)> = Vec::new();
    for (core, custom) in caps {
        let PhpKey::Str(core) = core else { continue };
        let Some(core) = META_CAPS.iter().find(|m| m.as_bytes() == core.as_slice()) else {
            continue;
        };
        // `$post_type_meta_caps[ $custom ]`: PHP's array key conversion. An array or object custom cap is
        // skipped (PHP throws "Illegal offset type").
        let Some(custom) = to_array_key(custom) else {
            continue;
        };
        let custom = key_to_string(&custom);
        match out.iter_mut().find(|(c, _)| *c == custom) {
            Some(entry) => entry.1 = core,
            None => out.push((custom, core)),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &str) -> PhpValue {
        PhpValue::Str(v.as_bytes().to_vec())
    }

    fn k(v: &str) -> PhpKey {
        PhpKey::Str(v.as_bytes().to_vec())
    }

    #[test]
    fn custom_caps_are_keyed_as_php_array_keys() {
        let caps = vec![
            (k("edit_post"), PhpValue::Bool(false)),
            (k("read_post"), PhpValue::Float(1.7)),
            (k("delete_post"), s("7")),
        ];
        assert_eq!(
            meta_capabilities(&caps),
            vec![
                ("0".to_owned(), "edit_post"),
                ("1".to_owned(), "read_post"),
                ("7".to_owned(), "delete_post")
            ]
        );
        // true and 1.7 are both key 1: the later core wins, in the first place.
        let caps = vec![
            (k("edit_post"), PhpValue::Bool(true)),
            (k("read_post"), PhpValue::Float(1.7)),
        ];
        assert_eq!(
            meta_capabilities(&caps),
            vec![("1".to_owned(), "read_post")]
        );
    }

    #[test]
    fn a_shared_custom_cap_keeps_its_first_place_and_last_core() {
        let caps = vec![
            (k("edit_post"), s("manage")),
            (k("read_post"), s("read")),
            (k("delete_post"), s("manage")),
        ];
        assert_eq!(
            meta_capabilities(&caps),
            vec![
                ("manage".to_owned(), "delete_post"),
                ("read".to_owned(), "read_post")
            ]
        );
    }
}
