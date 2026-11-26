//! Post type labels: `WP_Post_Type::get_default_labels()`, `_get_custom_object_labels()` and
//! `get_post_type_labels()`, without the `post_type_labels_{$post_type}` filter, which the caller applies between
//! [`post_type_labels`] and [`merge_filtered_labels`].

use speedyspec_php_serialize::semantics::{
    array_merge, get, is_empty, isset, set, to_array, to_php_string,
};
use speedyspec_php_serialize::{PhpKey, PhpValue};
use speedyspec_wp_posts_contracts::Translator;

/// A default label: `[non-hierarchical, hierarchical]`. `None` is PHP's null.
pub type LabelPair = [Option<String>; 2];

/// A label's English text and its translation context (`_x()`), or none (`__()`).
type Text = (&'static str, Option<&'static str>);

const GENERAL: Option<&str> = Some("post type general name");
const SINGULAR: Option<&str> = Some("post type singular name");
const LINK_TITLE: Option<&str> = Some("navigation link block title");
const LINK_DESCRIPTION: Option<&str> = Some("navigation link block description");

/// `get_default_labels()`, 7.2-RC2 lines 1000-1040: `[non-hierarchical, hierarchical]`; `None` is null.
const DEFAULT_LABELS: [(&str, [Option<Text>; 2]); 33] = [
    ("name", [Some(("Posts", GENERAL)), Some(("Pages", GENERAL))]),
    (
        "singular_name",
        [Some(("Post", SINGULAR)), Some(("Page", SINGULAR))],
    ),
    ("add_new", [Some(("Add", None)), Some(("Add", None))]),
    (
        "add_new_item",
        [Some(("Add Post", None)), Some(("Add Page", None))],
    ),
    (
        "edit_item",
        [Some(("Edit Post", None)), Some(("Edit Page", None))],
    ),
    (
        "new_item",
        [Some(("New Post", None)), Some(("New Page", None))],
    ),
    (
        "view_item",
        [Some(("View Post", None)), Some(("View Page", None))],
    ),
    (
        "view_items",
        [Some(("View Posts", None)), Some(("View Pages", None))],
    ),
    (
        "search_items",
        [Some(("Search Posts", None)), Some(("Search Pages", None))],
    ),
    (
        "not_found",
        [
            Some(("No posts found.", None)),
            Some(("No pages found.", None)),
        ],
    ),
    (
        "not_found_in_trash",
        [
            Some(("No posts found in Trash.", None)),
            Some(("No pages found in Trash.", None)),
        ],
    ),
    ("parent_item_colon", [None, Some(("Parent Page:", None))]),
    (
        "all_items",
        [Some(("All Posts", None)), Some(("All Pages", None))],
    ),
    (
        "archives",
        [Some(("Post Archives", None)), Some(("Page Archives", None))],
    ),
    (
        "attributes",
        [
            Some(("Post Attributes", None)),
            Some(("Page Attributes", None)),
        ],
    ),
    (
        "insert_into_item",
        [
            Some(("Insert into post", None)),
            Some(("Insert into page", None)),
        ],
    ),
    (
        "uploaded_to_this_item",
        [
            Some(("Uploaded to this post", None)),
            Some(("Uploaded to this page", None)),
        ],
    ),
    (
        "featured_image",
        [
            Some(("Featured image", Some("post"))),
            Some(("Featured image", Some("page"))),
        ],
    ),
    (
        "set_featured_image",
        [
            Some(("Set featured image", Some("post"))),
            Some(("Set featured image", Some("page"))),
        ],
    ),
    (
        "remove_featured_image",
        [
            Some(("Remove featured image", Some("post"))),
            Some(("Remove featured image", Some("page"))),
        ],
    ),
    (
        "use_featured_image",
        [
            Some(("Use as featured image", Some("post"))),
            Some(("Use as featured image", Some("page"))),
        ],
    ),
    (
        "filter_items_list",
        [
            Some(("Filter posts list", None)),
            Some(("Filter pages list", None)),
        ],
    ),
    (
        "filter_by_date",
        [
            Some(("Filter by date", None)),
            Some(("Filter by date", None)),
        ],
    ),
    (
        "items_list_navigation",
        [
            Some(("Posts list navigation", None)),
            Some(("Pages list navigation", None)),
        ],
    ),
    (
        "items_list",
        [Some(("Posts list", None)), Some(("Pages list", None))],
    ),
    (
        "item_published",
        [
            Some(("Post published.", None)),
            Some(("Page published.", None)),
        ],
    ),
    (
        "item_published_privately",
        [
            Some(("Post published privately.", None)),
            Some(("Page published privately.", None)),
        ],
    ),
    (
        "item_reverted_to_draft",
        [
            Some(("Post reverted to draft.", None)),
            Some(("Page reverted to draft.", None)),
        ],
    ),
    (
        "item_trashed",
        [Some(("Post trashed.", None)), Some(("Page trashed.", None))],
    ),
    (
        "item_scheduled",
        [
            Some(("Post scheduled.", None)),
            Some(("Page scheduled.", None)),
        ],
    ),
    (
        "item_updated",
        [Some(("Post updated.", None)), Some(("Page updated.", None))],
    ),
    (
        "item_link",
        [
            Some(("Post Link", LINK_TITLE)),
            Some(("Page Link", LINK_TITLE)),
        ],
    ),
    (
        "item_link_description",
        [
            Some(("A link to a post.", LINK_DESCRIPTION)),
            Some(("A link to a page.", LINK_DESCRIPTION)),
        ],
    ),
];

/// `WP_Post_Type::get_default_labels()`, in order, each through the translator with its context.
pub fn default_labels(t: &dyn Translator) -> Vec<(&'static str, LabelPair)> {
    let translate = |text: &Option<Text>| text.map(|(text, context)| t.translate(text, context));
    DEFAULT_LABELS
        .iter()
        .map(|(key, [flat, hierarchical])| (*key, [translate(flat), translate(hierarchical)]))
        .collect()
}

/// What `get_post_type_labels()` reads from the post type object.
pub struct LabelSource<'a> {
    /// `$post_type_object->name`.
    pub name: &'a str,
    /// `$post_type_object->label` when it is set (`isset()`: present and not null).
    pub label: Option<&'a PhpValue>,
    /// `$post_type_object->labels`, as given; it is cast to an array.
    pub labels: &'a PhpValue,
    /// Whether `$post_type_object->hierarchical` is truthy.
    pub hierarchical: bool,
}

/// What `_get_custom_object_labels()` produces.
#[derive(Debug, Clone, PartialEq)]
pub struct CustomLabels {
    /// `$data_object->labels` after its fill-ins (`name`, `singular_name`, `name_admin_bar`, `menu_name`,
    /// `all_items`, `archives`).
    pub given: Vec<(PhpKey, PhpValue)>,
    /// The defaults for the object's hierarchy merged with `given`.
    pub labels: Vec<(PhpKey, PhpValue)>,
}

/// `_get_custom_object_labels()`.
pub fn custom_object_labels(
    source: &LabelSource<'_>,
    defaults: &[(&str, LabelPair)],
) -> CustomLabels {
    let mut given = to_array(source.labels.clone());

    if let Some(label) = source.label.filter(|v| !matches!(v, PhpValue::Null))
        && get(&given, "name").is_none_or(is_empty)
    {
        set(&mut given, "name", label.clone());
    }
    if isset(&given, "singular_name").is_none()
        && let Some(name) = isset(&given, "name").cloned()
    {
        set(&mut given, "singular_name", name);
    }
    if isset(&given, "name_admin_bar").is_none() {
        let bar = isset(&given, "singular_name")
            .cloned()
            .unwrap_or_else(|| PhpValue::Str(source.name.as_bytes().to_vec()));
        set(&mut given, "name_admin_bar", bar);
    }
    // Each fill-in reads the one before it: menu_name from name, all_items from menu_name, archives from all_items.
    for (missing, from) in [
        ("menu_name", "name"),
        ("all_items", "menu_name"),
        ("archives", "all_items"),
    ] {
        if isset(&given, missing).is_none()
            && let Some(value) = isset(&given, from).cloned()
        {
            set(&mut given, missing, value);
        }
    }

    let side = usize::from(source.hierarchical);
    let defaults = defaults
        .iter()
        .map(|(key, pair)| {
            let value = pair[side].as_ref().map_or(PhpValue::Null, |text| {
                PhpValue::Str(text.as_bytes().to_vec())
            });
            (PhpKey::Str(key.as_bytes().to_vec()), value)
        })
        .collect();
    let labels = array_merge(defaults, given.clone());
    CustomLabels { given, labels }
}

/// `get_post_type_labels()` before its `post_type_labels_{$post_type}` filter.
pub fn post_type_labels(source: &LabelSource<'_>, t: &dyn Translator) -> Vec<(PhpKey, PhpValue)> {
    let mut defaults = default_labels(t);
    let name = defaults
        .iter()
        .find(|(key, _)| *key == "name")
        .map(|(_, pair)| pair.clone())
        .expect("the defaults have a name");
    defaults.push(("menu_name", name));

    let CustomLabels { given, mut labels } = custom_object_labels(source, &defaults);
    if isset(&given, "template_name").is_none()
        && let Some(singular) = isset(&given, "singular_name")
    {
        /* translators: %s: Post type name. */
        let format = t.translate("Single item: %s", None);
        let template = sprintf(format.as_bytes(), &to_php_string(singular));
        set(&mut labels, "template_name", PhpValue::Str(template));
    }
    labels
}

/// `sprintf()` with one string argument: `%s`, `%1$s` and `%%`. Anything else is copied as written.
fn sprintf(format: &[u8], arg: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(format.len() + arg.len());
    let mut i = 0;
    while i < format.len() {
        if format[i] == b'%' {
            match &format[i + 1..] {
                [b'%', ..] => {
                    out.push(b'%');
                    i += 2;
                    continue;
                }
                [b's', ..] => {
                    out.extend_from_slice(arg);
                    i += 2;
                    continue;
                }
                [b'1', b'$', b's', ..] => {
                    out.extend_from_slice(arg);
                    i += 4;
                    continue;
                }
                _ => {}
            }
        }
        out.push(format[i]);
        i += 1;
    }
    out
}

/// `(object) array_merge( (array) $default_labels, (array) $labels )`, after the filter.
pub fn merge_filtered_labels(
    defaults: Vec<(PhpKey, PhpValue)>,
    filtered: Vec<(PhpKey, PhpValue)>,
) -> Vec<(PhpKey, PhpValue)> {
    array_merge(defaults, filtered)
}

/// A [`Translator`] that answers WordPress's English.
#[derive(Debug, Clone, Copy, Default)]
pub struct Untranslated;

impl Translator for Untranslated {
    fn translate(&self, text: &str, _context: Option<&str>) -> String {
        text.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Answers `"[{context}]{text}"`.
    struct Bracketed;
    impl Translator for Bracketed {
        fn translate(&self, text: &str, context: Option<&str>) -> String {
            format!("[{}]{text}", context.unwrap_or(""))
        }
    }

    fn s(v: &str) -> PhpValue {
        PhpValue::Str(v.as_bytes().to_vec())
    }

    fn k(v: &str) -> PhpKey {
        PhpKey::Str(v.as_bytes().to_vec())
    }

    fn lookup<'a>(items: &'a [(PhpKey, PhpValue)], name: &str) -> Option<&'a PhpValue> {
        items
            .iter()
            .find(|(key, _)| *key == k(name))
            .map(|(_, v)| v)
    }

    #[test]
    fn labels_go_through_the_translator() {
        let defaults = default_labels(&Bracketed);
        let singular = defaults
            .iter()
            .find(|(key, _)| *key == "singular_name")
            .unwrap();
        assert_eq!(
            singular.1[0].as_deref(),
            Some("[post type singular name]Post")
        );
        let add_new = defaults.iter().find(|(key, _)| *key == "add_new").unwrap();
        assert_eq!(add_new.1[0].as_deref(), Some("[]Add"));
        let parent = defaults
            .iter()
            .find(|(key, _)| *key == "parent_item_colon")
            .unwrap();
        assert_eq!(parent.1, [None, Some("[]Parent Page:".to_owned())]);
        assert_eq!(defaults.len(), 33);

        let labels = PhpValue::Array(vec![(k("singular_name"), s("Book"))]);
        let source = LabelSource {
            name: "book",
            label: None,
            labels: &labels,
            hierarchical: false,
        };
        let out = post_type_labels(&source, &Bracketed);
        assert_eq!(
            lookup(&out, "template_name"),
            Some(&s("[]Single item: Book"))
        );
        assert_eq!(out.last().map(|(key, _)| key), Some(&k("template_name")));
        // No given name: menu_name keeps the default name pair, translated with its context.
        assert_eq!(
            lookup(&out, "menu_name"),
            Some(&s("[post type general name]Posts"))
        );
        assert_eq!(lookup(&out, "name_admin_bar"), Some(&s("Book")));
    }

    #[test]
    fn template_name_formats_positional_and_percent() {
        struct Positional;
        impl Translator for Positional {
            fn translate(&self, text: &str, _context: Option<&str>) -> String {
                if text == "Single item: %s" {
                    "100%% %1$s".to_owned()
                } else {
                    text.to_owned()
                }
            }
        }
        let labels = PhpValue::Array(vec![(k("singular_name"), s("Book"))]);
        let source = LabelSource {
            name: "book",
            label: None,
            labels: &labels,
            hierarchical: false,
        };
        let out = post_type_labels(&source, &Positional);
        assert_eq!(lookup(&out, "template_name"), Some(&s("100% Book")));
    }

    #[test]
    fn merge_filtered_labels_keeps_missing_defaults() {
        let defaults = vec![(k("name"), s("Books")), (k("add_new"), s("Add"))];
        let filtered = vec![(k("name"), s("Novels")), (k("extra"), s("x"))];
        assert_eq!(
            merge_filtered_labels(defaults, filtered),
            vec![
                (k("name"), s("Novels")),
                (k("add_new"), s("Add")),
                (k("extra"), s("x"))
            ]
        );
    }
}
