//! Post statuses: `register_post_status()`'s argument resolution and `$wp_post_statuses` (post.php, 7.1.5 lines
//! 1457-1530), `get_post_status_object()` (1552-1560), and the label tables of `get_post_statuses()`,
//! `get_page_statuses()` and `_wp_privacy_statuses()` (1360-1406).
//!
//! `$args` is an array here: `wp_parse_args()`' query-string form is the adapter's to parse. The adapter also
//! answers `get_post_status_object()`'s `! is_string()` check; a stored status is an object, never `empty()`.

use indexmap::IndexMap;
use speedyspec_php_serialize::semantics::{array_merge, is_empty};
use speedyspec_php_serialize::{PhpKey, PhpValue};
use speedyspec_wp_posts_contracts::{PostStatusName, PostStatusObject, PostStatuses, Translator};

use crate::key::sanitize_key;

/// `register_post_status()`'s defaults, in order (1465-1478).
fn defaults() -> Vec<(PhpKey, PhpValue)> {
    [
        ("label", PhpValue::Bool(false)),
        ("label_count", PhpValue::Bool(false)),
        ("exclude_from_search", PhpValue::Null),
        ("_builtin", PhpValue::Bool(false)),
        ("public", PhpValue::Null),
        ("internal", PhpValue::Null),
        ("protected", PhpValue::Null),
        ("private", PhpValue::Null),
        ("publicly_queryable", PhpValue::Null),
        ("show_in_admin_status_list", PhpValue::Null),
        ("show_in_admin_all_list", PhpValue::Null),
        ("date_floating", PhpValue::Null),
    ]
    .into_iter()
    .map(|(key, value)| (PhpKey::Str(key.as_bytes().to_vec()), value))
    .collect()
}

/// `null === $args->prop`. Every checked property has a default, so a missing one does not occur.
fn is_null(object: &PostStatusObject, prop: &str) -> bool {
    matches!(object.get(prop), None | Some(PhpValue::Null))
}

/// `null === $args->prop` → `$args->prop = $value`.
fn default_to(object: &mut PostStatusObject, prop: &str, value: PhpValue) {
    if is_null(object, prop) {
        object.set(prop, value);
    }
}

/// `! $value` for the bools `register_post_status()` negates.
fn not(value: Option<&PhpValue>) -> PhpValue {
    PhpValue::Bool(value.is_none_or(is_empty))
}

/// `_n_noop( $singular, $plural )`.
fn n_noop(singular: PhpValue, plural: PhpValue) -> PhpValue {
    let key = |k: &str| PhpKey::Str(k.as_bytes().to_vec());
    PhpValue::Array(vec![
        (PhpKey::Int(0), singular.clone()),
        (PhpKey::Int(1), plural.clone()),
        (key("singular"), singular),
        (key("plural"), plural),
        (key("context"), PhpValue::Null),
        (key("domain"), PhpValue::Null),
    ])
}

/// `register_post_status()`'s `$args` object: `array_merge( $defaults, $args )` cast to an object, `name` set (in
/// place when `$args` gave one, else appended), then the null defaults resolved in WordPress's order.
pub fn resolve_post_status(
    name: PostStatusName,
    args: Vec<(PhpKey, PhpValue)>,
) -> PostStatusObject {
    let mut object = PostStatusObject::new(name.clone(), array_merge(defaults(), args));
    let name_value = PhpValue::Str(name.as_str().as_bytes().to_vec());
    object.set("name", name_value.clone());

    if ["public", "internal", "protected", "private"]
        .iter()
        .all(|prop| is_null(&object, prop))
    {
        object.set("internal", PhpValue::Bool(true));
    }
    for prop in ["public", "private", "protected", "internal"] {
        default_to(&mut object, prop, PhpValue::Bool(false));
    }
    let public = object.get("public").cloned().unwrap_or(PhpValue::Null);
    default_to(&mut object, "publicly_queryable", public);
    let internal = object.get("internal").cloned();
    default_to(
        &mut object,
        "exclude_from_search",
        internal.clone().unwrap_or(PhpValue::Null),
    );
    default_to(
        &mut object,
        "show_in_admin_all_list",
        not(internal.as_ref()),
    );
    default_to(
        &mut object,
        "show_in_admin_status_list",
        not(internal.as_ref()),
    );
    default_to(&mut object, "date_floating", PhpValue::Bool(false));

    if matches!(object.get("label"), Some(PhpValue::Bool(false))) {
        object.set("label", name_value);
    }
    if matches!(object.get("label_count"), Some(PhpValue::Bool(false))) {
        let label = object.get("label").cloned().unwrap_or(PhpValue::Null);
        object.set("label_count", n_noop(label.clone(), label));
    }
    object
}

/// `$wp_post_statuses`, in registration order.
#[derive(Clone, Debug, Default)]
pub struct PostStatusRegistry {
    statuses: IndexMap<String, PostStatusObject>,
}

impl PostStatusRegistry {
    /// An empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// `register_post_status()`: `sanitize_key()` with no length check (`''` is stored), resolves the object, and
    /// stores it in place when the name is already registered, else appends it.
    pub fn register(&mut self, raw_name: &[u8], args: Vec<(PhpKey, PhpValue)>) -> PostStatusObject {
        let name = sanitize_key(raw_name);
        let object = resolve_post_status(PostStatusName::new(name.clone()), args);
        self.statuses.insert(name, object.clone());
        object
    }
}

impl PostStatuses for PostStatusRegistry {
    /// `get_post_status_object()` for a string name.
    fn status(&self, name: &str) -> Option<PostStatusObject> {
        self.statuses.get(name).cloned()
    }

    fn statuses(&self) -> Vec<PostStatusObject> {
        self.statuses.values().cloned().collect()
    }
}

/// `get_post_statuses()`.
pub fn post_statuses(t: &dyn Translator) -> Vec<(&'static str, String)> {
    vec![
        ("draft", t.translate("Draft", None)),
        ("pending", t.translate("Pending Review", None)),
        ("private", t.translate("Private", None)),
        ("publish", t.translate("Published", None)),
    ]
}

/// `get_page_statuses()`: the post statuses without `pending`.
pub fn page_statuses(t: &dyn Translator) -> Vec<(&'static str, String)> {
    vec![
        ("draft", t.translate("Draft", None)),
        ("private", t.translate("Private", None)),
        ("publish", t.translate("Published", None)),
    ]
}

/// `_wp_privacy_statuses()`, each label in the `request status` context.
pub fn privacy_statuses(t: &dyn Translator) -> Vec<(&'static str, String)> {
    const CONTEXT: Option<&str> = Some("request status");
    vec![
        ("request-pending", t.translate("Pending", CONTEXT)),
        ("request-confirmed", t.translate("Confirmed", CONTEXT)),
        ("request-failed", t.translate("Failed", CONTEXT)),
        ("request-completed", t.translate("Completed", CONTEXT)),
    ]
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
    fn all_null_flags_make_internal() {
        let status = resolve_post_status(PostStatusName::new("archived"), Vec::new());
        assert_eq!(status.get("internal"), Some(&PhpValue::Bool(true)));
        for prop in ["public", "protected", "private", "publicly_queryable"] {
            assert_eq!(status.get(prop), Some(&PhpValue::Bool(false)), "{prop}");
        }
        assert_eq!(
            status.get("exclude_from_search"),
            Some(&PhpValue::Bool(true))
        );
        assert_eq!(
            status.get("show_in_admin_all_list"),
            Some(&PhpValue::Bool(false))
        );
        assert_eq!(
            status.get("show_in_admin_status_list"),
            Some(&PhpValue::Bool(false))
        );

        // An explicit null still counts as unset; any one flag given stops the internal default.
        let status = resolve_post_status(
            PostStatusName::new("archived"),
            vec![
                (k("public"), PhpValue::Null),
                (k("private"), PhpValue::Bool(true)),
            ],
        );
        assert_eq!(status.get("internal"), Some(&PhpValue::Bool(false)));
        assert_eq!(status.get("public"), Some(&PhpValue::Bool(false)));
        assert_eq!(
            status.get("show_in_admin_all_list"),
            Some(&PhpValue::Bool(true))
        );
    }

    #[test]
    fn empty_name_is_stored() {
        let mut registry = PostStatusRegistry::new();
        let status = registry.register(b"!!!", Vec::new());
        assert_eq!(status.name().as_str(), "");
        assert_eq!(status.get("name"), Some(&s("")));
        assert_eq!(status.get("label"), Some(&s("")));
        assert!(registry.status("").is_some());
        assert_eq!(registry.statuses().len(), 1);

        let odd = registry.register(b"Odd Status!", Vec::new());
        assert_eq!(odd.name().as_str(), "oddstatus");
        assert!(registry.status("oddstatus").is_some());
    }

    #[test]
    fn label_count_defaults_to_the_label() {
        let status = resolve_post_status(
            PostStatusName::new("archived"),
            vec![(k("label"), s("Archived"))],
        );
        assert_eq!(
            status.get("label_count"),
            Some(&PhpValue::Array(vec![
                (PhpKey::Int(0), s("Archived")),
                (PhpKey::Int(1), s("Archived")),
                (k("singular"), s("Archived")),
                (k("plural"), s("Archived")),
                (k("context"), PhpValue::Null),
                (k("domain"), PhpValue::Null),
            ]))
        );
        // A given label_count stays as it is.
        let status = resolve_post_status(
            PostStatusName::new("archived"),
            vec![(k("label_count"), s("custom"))],
        );
        assert_eq!(status.get("label_count"), Some(&s("custom")));
        assert_eq!(status.get("label"), Some(&s("archived")));
    }

    #[test]
    fn re_register_keeps_position() {
        let mut registry = PostStatusRegistry::new();
        registry.register(b"first", vec![(k("public"), PhpValue::Bool(true))]);
        registry.register(b"second", Vec::new());
        registry.register(b"first", vec![(k("private"), PhpValue::Bool(true))]);
        let names: Vec<String> = registry
            .statuses()
            .iter()
            .map(|s| s.name().as_str().to_owned())
            .collect();
        assert_eq!(names, ["first", "second"]);
        assert_eq!(
            registry.status("first").unwrap().get("private"),
            Some(&PhpValue::Bool(true))
        );
    }
}
