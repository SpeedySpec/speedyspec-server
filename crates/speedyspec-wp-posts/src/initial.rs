//! `create_initial_post_types()` (post.php, 7.2-RC2 lines 20-834) as a table: each `register_post_type()` call in
//! WordPress's order with its args as PHP arrays in the source's key order, its labels through the translator, the
//! supports calls WordPress makes between them, then each `register_post_status()` call in order.
//!
//! The table is a list of [`InitialCall`]s (Command): the function only runs them against the registries, so the order
//! and data live in one place. `WP_Post_Type::reset_default_labels()` has nothing to reset here: the default labels
//! are built per call. `build_query()` does not encode, so the two site editor edit links are constants.

use speedyspec_php_serialize::{PhpKey, PhpValue};
use speedyspec_wp_posts_contracts::{PostTypeName, Translator};

use crate::post_type::PostTypePorts;
use crate::registry::{PostTypeRegistration, PostTypeRegistry};
use crate::status::{PostStatusRegistry, n_noop};

/// The options `create_initial_post_types()` reads.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct InitialOptions {
    /// `'1' === get_option( 'wp_attachment_pages_enabled' )`.
    pub attachment_pages_enabled: bool,
}

/// `'site-editor.php?' . build_query( array( 'p' => '/%s/%s', 'canvas' => 'edit' ) )`.
const TEMPLATE_EDIT_LINK: &str = "site-editor.php?p=/%s/%s&canvas=edit";
/// `'site-editor.php?' . build_query( array( 'p' => '/wp_navigation/%s', 'canvas' => 'edit' ) )`.
const NAVIGATION_EDIT_LINK: &str = "site-editor.php?p=/wp_navigation/%s&canvas=edit";

/// One call `create_initial_post_types()` makes.
#[derive(Debug, Clone)]
pub enum InitialCall {
    /// `register_post_type( $name, $args )`.
    PostType(&'static str, Vec<(PhpKey, PhpValue)>),
    /// `add_post_type_support( $post_type, $feature )`.
    AddSupport(&'static str, &'static str),
    /// `remove_post_type_support( $post_type, $feature )`.
    RemoveSupport(&'static str, &'static str),
    /// `register_post_status( $name, $args )`.
    Status(&'static str, Vec<(PhpKey, PhpValue)>),
}

/// `create_initial_post_types()`: runs each call in WordPress's order and returns each post type's registration,
/// whose actions (`future_{$post_type}`) and taxonomies the adapter applies.
pub fn create_initial_post_types(
    types: &mut PostTypeRegistry,
    statuses: &mut PostStatusRegistry,
    options: &InitialOptions,
    ports: &PostTypePorts<'_>,
) -> Vec<PostTypeRegistration> {
    let mut registrations = Vec::new();
    for step in initial_calls(options, ports.translator) {
        match step {
            InitialCall::PostType(name, args) => {
                registrations.push(types.register(PostTypeName::new(name), args, ports));
            }
            InitialCall::AddSupport(post_type, feature) => {
                types.add_support(post_type, &[feature], Vec::new());
            }
            InitialCall::RemoveSupport(post_type, feature) => {
                types.remove_support(post_type, feature)
            }
            InitialCall::Status(name, args) => {
                statuses.register(name.as_bytes(), args);
            }
        }
    }
    registrations
}

/// A PHP array literal, keys assigned as PHP assigns them: [`Arr::push`] takes the next integer key.
#[derive(Default)]
struct Arr {
    items: Vec<(PhpKey, PhpValue)>,
    next: i64,
}

impl Arr {
    fn new() -> Self {
        Self::default()
    }

    /// `'key' => $value`.
    fn set(mut self, key: &str, value: impl Into<Value>) -> Self {
        self.items
            .push((PhpKey::Str(key.as_bytes().to_vec()), value.into().0));
        self
    }

    /// `$value` with the next integer key.
    fn push(mut self, value: impl Into<Value>) -> Self {
        self.items.push((PhpKey::Int(self.next), value.into().0));
        self.next += 1;
        self
    }

    fn items(self) -> Vec<(PhpKey, PhpValue)> {
        self.items
    }
}

/// A value in the table: a string, bool, int or nested array.
struct Value(PhpValue);

impl From<&str> for Value {
    fn from(v: &str) -> Self {
        Self(PhpValue::Str(v.as_bytes().to_vec()))
    }
}

impl From<String> for Value {
    fn from(v: String) -> Self {
        Self(PhpValue::Str(v.into_bytes()))
    }
}

impl From<bool> for Value {
    fn from(v: bool) -> Self {
        Self(PhpValue::Bool(v))
    }
}

impl From<i64> for Value {
    fn from(v: i64) -> Self {
        Self(PhpValue::Int(v))
    }
}

impl From<Arr> for Value {
    fn from(v: Arr) -> Self {
        Self(PhpValue::Array(v.items))
    }
}

/// `array( 'a', 'b', ... )`.
fn list(values: &[&str]) -> Arr {
    values.iter().fold(Arr::new(), |arr, v| arr.push(*v))
}

/// `array( 'key' => 'value', ... )`.
fn pairs(entries: &[(&str, &str)]) -> Arr {
    entries
        .iter()
        .fold(Arr::new(), |arr, (k, v)| arr.set(k, *v))
}

/// `array( 'key' => __( 'text' ), ... )`.
fn labels(t: &dyn Translator, entries: &[(&str, &str)]) -> Arr {
    entries
        .iter()
        .fold(Arr::new(), |arr, (k, v)| arr.set(k, t.translate(v, None)))
}

/// `_x( $text, $context )`.
fn x(t: &dyn Translator, text: &str, context: &str) -> String {
    t.translate(text, Some(context))
}

/// `__( $text )`.
fn tr(t: &dyn Translator, text: &str) -> String {
    t.translate(text, None)
}

/// `capabilities` that map every listed capability to `edit_theme_options`.
fn theme_options(caps: &[&str]) -> Arr {
    caps.iter()
        .fold(Arr::new(), |arr, cap| arr.set(cap, "edit_theme_options"))
}

/// `_n_noop( $singular, $plural )`, for a status's `label_count`.
fn count(singular: &str, plural: &str) -> Value {
    Value(n_noop(
        PhpValue::Str(singular.as_bytes().to_vec()),
        PhpValue::Str(plural.as_bytes().to_vec()),
    ))
}

/// Every call `create_initial_post_types()` makes, in WordPress's order, for an adapter that makes them itself (the
/// PHP one runs each through the native `register_post_type()` and the rest, so their hooks fire).
pub fn initial_calls(options: &InitialOptions, t: &dyn Translator) -> Vec<InitialCall> {
    let mut steps = post_type_steps(options, t);
    steps.extend(status_steps(t));
    steps
}

/// The editor feature's `array( 'notes' => true )` for posts and pages.
fn editor_notes() -> Arr {
    Arr::new().set("notes", true)
}

fn post_type_steps(options: &InitialOptions, t: &dyn Translator) -> Vec<InitialCall> {
    // wp_template and wp_template_part give the same capabilities.
    let template_caps = || {
        theme_options(&[
            "create_posts",
            "delete_posts",
            "delete_others_posts",
            "delete_private_posts",
            "delete_published_posts",
            "edit_posts",
            "edit_others_posts",
            "edit_private_posts",
            "edit_published_posts",
            "publish_posts",
            "read",
            "read_private_posts",
        ])
    };
    let font_caps = || {
        theme_options(&[
            "read",
            "read_private_posts",
            "create_posts",
            "publish_posts",
            "edit_posts",
            "edit_others_posts",
            "edit_published_posts",
            "delete_posts",
            "delete_others_posts",
            "delete_published_posts",
        ])
    };
    let template_supports = || list(&["title", "slug", "excerpt", "editor", "revisions", "author"]);

    vec![
        InitialCall::PostType(
            "post",
            Arr::new()
                .set(
                    "labels",
                    Arr::new().set("name_admin_bar", x(t, "Post", "add new from admin bar")),
                )
                .set("public", true)
                .set("randomable", true)
                .set("_builtin", true)
                .set("_edit_link", "post.php?post=%d")
                .set("capability_type", "post")
                .set("map_meta_cap", true)
                .set("menu_position", 5)
                .set("menu_icon", "dashicons-admin-post")
                .set("hierarchical", false)
                .set("rewrite", false)
                .set("query_var", false)
                .set("delete_with_user", true)
                .set(
                    "supports",
                    Arr::new()
                        .push("title")
                        .set("editor", editor_notes())
                        .push("author")
                        .push("thumbnail")
                        .push("excerpt")
                        .push("trackbacks")
                        .push("custom-fields")
                        .push("comments")
                        .push("revisions")
                        .push("post-formats"),
                )
                .set("show_in_rest", true)
                .set("show_in_abilities", true)
                .set("rest_base", "posts")
                .set("rest_controller_class", "WP_REST_Posts_Controller")
                .items(),
        ),
        InitialCall::PostType(
            "page",
            Arr::new()
                .set(
                    "labels",
                    Arr::new().set("name_admin_bar", x(t, "Page", "add new from admin bar")),
                )
                .set("public", true)
                .set("publicly_queryable", false)
                .set("randomable", false)
                .set("_builtin", true)
                .set("_edit_link", "post.php?post=%d")
                .set("capability_type", "page")
                .set("map_meta_cap", true)
                .set("menu_position", 20)
                .set("menu_icon", "dashicons-admin-page")
                .set("hierarchical", true)
                .set("rewrite", false)
                .set("query_var", false)
                .set("delete_with_user", true)
                .set(
                    "supports",
                    Arr::new()
                        .push("title")
                        .set("editor", editor_notes())
                        .push("author")
                        .push("thumbnail")
                        .push("page-attributes")
                        .push("custom-fields")
                        .push("comments")
                        .push("revisions"),
                )
                .set("show_in_rest", true)
                .set("show_in_abilities", true)
                .set("rest_base", "pages")
                .set("rest_controller_class", "WP_REST_Posts_Controller")
                .items(),
        ),
        InitialCall::PostType(
            "attachment",
            Arr::new()
                .set(
                    "labels",
                    Arr::new()
                        .set("name", x(t, "Media", "post type general name"))
                        .set("name_admin_bar", x(t, "Media", "add new from admin bar"))
                        .set("add_new", tr(t, "Add Media File"))
                        .set("add_new_item", tr(t, "Add Media File"))
                        .set("edit_item", tr(t, "Edit Media"))
                        .set(
                            "view_item",
                            if options.attachment_pages_enabled {
                                tr(t, "View Attachment Page")
                            } else {
                                tr(t, "View Media File")
                            },
                        )
                        .set("attributes", tr(t, "Attachment Attributes")),
                )
                .set("public", true)
                .set("randomable", false)
                .set("show_ui", true)
                .set("_builtin", true)
                .set("_edit_link", "post.php?post=%d")
                .set("capability_type", "post")
                .set("capabilities", pairs(&[("create_posts", "upload_files")]))
                .set("map_meta_cap", true)
                .set("menu_icon", "dashicons-admin-media")
                .set("hierarchical", false)
                .set("rewrite", false)
                .set("query_var", false)
                .set("show_in_nav_menus", false)
                .set("delete_with_user", true)
                .set("supports", list(&["title", "author", "comments"]))
                .set("show_in_rest", true)
                .set("rest_base", "media")
                .set("rest_controller_class", "WP_REST_Attachments_Controller")
                .items(),
        ),
        InitialCall::AddSupport("attachment:audio", "thumbnail"),
        InitialCall::AddSupport("attachment:video", "thumbnail"),
        InitialCall::PostType(
            "revision",
            Arr::new()
                .set(
                    "labels",
                    labels(t, &[("name", "Revisions"), ("singular_name", "Revision")]),
                )
                .set("public", false)
                .set("_builtin", true)
                .set("_edit_link", "revision.php?revision=%d")
                .set("capability_type", "post")
                .set("map_meta_cap", true)
                .set("hierarchical", false)
                .set("rewrite", false)
                .set("query_var", false)
                .set("can_export", false)
                .set("delete_with_user", true)
                .set("supports", list(&["author"]))
                .items(),
        ),
        InitialCall::PostType(
            "nav_menu_item",
            Arr::new()
                .set(
                    "labels",
                    labels(
                        t,
                        &[
                            ("name", "Navigation Menu Items"),
                            ("singular_name", "Navigation Menu Item"),
                        ],
                    ),
                )
                .set("public", false)
                .set("_builtin", true)
                .set("hierarchical", false)
                .set("rewrite", false)
                .set("delete_with_user", false)
                .set("query_var", false)
                .set("map_meta_cap", true)
                .set(
                    "capability_type",
                    list(&["edit_theme_options", "edit_theme_options"]),
                )
                .set(
                    "capabilities",
                    pairs(&[
                        ("edit_post", "edit_post"),
                        ("read_post", "read_post"),
                        ("delete_post", "delete_post"),
                        ("edit_posts", "edit_theme_options"),
                        ("edit_others_posts", "edit_theme_options"),
                        ("delete_posts", "edit_theme_options"),
                        ("publish_posts", "edit_theme_options"),
                        ("read_private_posts", "edit_theme_options"),
                        ("read", "read"),
                        ("delete_private_posts", "edit_theme_options"),
                        ("delete_published_posts", "edit_theme_options"),
                        ("delete_others_posts", "edit_theme_options"),
                        ("edit_private_posts", "edit_theme_options"),
                        ("edit_published_posts", "edit_theme_options"),
                    ]),
                )
                .set("show_in_rest", true)
                .set("rest_base", "menu-items")
                .set("rest_controller_class", "WP_REST_Menu_Items_Controller")
                .items(),
        ),
        InitialCall::PostType(
            "custom_css",
            Arr::new()
                .set(
                    "labels",
                    labels(
                        t,
                        &[("name", "Custom CSS"), ("singular_name", "Custom CSS")],
                    ),
                )
                .set("public", false)
                .set("hierarchical", false)
                .set("rewrite", false)
                .set("query_var", false)
                .set("delete_with_user", false)
                .set("can_export", true)
                .set("_builtin", true)
                .set("supports", list(&["title", "revisions"]))
                .set(
                    "capabilities",
                    pairs(&[
                        ("delete_posts", "edit_theme_options"),
                        ("delete_post", "edit_theme_options"),
                        ("delete_published_posts", "edit_theme_options"),
                        ("delete_private_posts", "edit_theme_options"),
                        ("delete_others_posts", "edit_theme_options"),
                        ("edit_post", "edit_css"),
                        ("edit_posts", "edit_css"),
                        ("edit_others_posts", "edit_css"),
                        ("edit_published_posts", "edit_css"),
                        ("read_post", "read"),
                        ("read_private_posts", "read"),
                        ("publish_posts", "edit_theme_options"),
                    ]),
                )
                .items(),
        ),
        InitialCall::PostType(
            "customize_changeset",
            Arr::new()
                .set(
                    "labels",
                    Arr::new()
                        .set("name", x(t, "Changesets", "post type general name"))
                        .set(
                            "singular_name",
                            x(t, "Changeset", "post type singular name"),
                        )
                        .set("add_new", tr(t, "Add Changeset"))
                        .set("add_new_item", tr(t, "Add Changeset"))
                        .set("new_item", tr(t, "New Changeset"))
                        .set("edit_item", tr(t, "Edit Changeset"))
                        .set("view_item", tr(t, "View Changeset"))
                        .set("all_items", tr(t, "All Changesets"))
                        .set("search_items", tr(t, "Search Changesets"))
                        .set("not_found", tr(t, "No changesets found."))
                        .set("not_found_in_trash", tr(t, "No changesets found in Trash.")),
                )
                .set("public", false)
                .set("_builtin", true)
                .set("map_meta_cap", true)
                .set("hierarchical", false)
                .set("rewrite", false)
                .set("query_var", false)
                .set("can_export", false)
                .set("delete_with_user", false)
                .set("supports", list(&["title", "author"]))
                .set("capability_type", "customize_changeset")
                .set(
                    "capabilities",
                    pairs(&[
                        ("create_posts", "customize"),
                        ("delete_others_posts", "customize"),
                        ("delete_post", "customize"),
                        ("delete_posts", "customize"),
                        ("delete_private_posts", "customize"),
                        ("delete_published_posts", "customize"),
                        ("edit_others_posts", "customize"),
                        ("edit_post", "customize"),
                        ("edit_posts", "customize"),
                        ("edit_private_posts", "customize"),
                        ("edit_published_posts", "do_not_allow"),
                        ("publish_posts", "customize"),
                        ("read", "read"),
                        ("read_post", "customize"),
                        ("read_private_posts", "customize"),
                    ]),
                )
                .items(),
        ),
        InitialCall::PostType(
            "oembed_cache",
            Arr::new()
                .set(
                    "labels",
                    labels(
                        t,
                        &[
                            ("name", "oEmbed Responses"),
                            ("singular_name", "oEmbed Response"),
                        ],
                    ),
                )
                .set("public", false)
                .set("hierarchical", false)
                .set("rewrite", false)
                .set("query_var", false)
                .set("delete_with_user", false)
                .set("can_export", false)
                .set("_builtin", true)
                .set("supports", Arr::new())
                .items(),
        ),
        InitialCall::PostType(
            "user_request",
            Arr::new()
                .set(
                    "labels",
                    labels(
                        t,
                        &[("name", "User Requests"), ("singular_name", "User Request")],
                    ),
                )
                .set("public", false)
                .set("_builtin", true)
                .set("hierarchical", false)
                .set("rewrite", false)
                .set("query_var", false)
                .set("can_export", false)
                .set("delete_with_user", false)
                .set("supports", Arr::new())
                .items(),
        ),
        InitialCall::PostType(
            "wp_block",
            Arr::new()
                .set(
                    "labels",
                    Arr::new()
                        .set("name", x(t, "Patterns", "post type general name"))
                        .set("singular_name", x(t, "Pattern", "post type singular name"))
                        .set("add_new", tr(t, "Add Pattern"))
                        .set("add_new_item", tr(t, "Add Pattern"))
                        .set("new_item", tr(t, "New Pattern"))
                        .set("edit_item", tr(t, "Edit Block Pattern"))
                        .set("view_item", tr(t, "View Pattern"))
                        .set("view_items", tr(t, "View Patterns"))
                        .set("all_items", tr(t, "All Patterns"))
                        .set("search_items", tr(t, "Search Patterns"))
                        .set("not_found", tr(t, "No patterns found."))
                        .set("not_found_in_trash", tr(t, "No patterns found in Trash."))
                        .set("filter_items_list", tr(t, "Filter patterns list"))
                        .set("items_list_navigation", tr(t, "Patterns list navigation"))
                        .set("items_list", tr(t, "Patterns list"))
                        .set("item_published", tr(t, "Pattern published."))
                        .set(
                            "item_published_privately",
                            tr(t, "Pattern published privately."),
                        )
                        .set(
                            "item_reverted_to_draft",
                            tr(t, "Pattern reverted to draft."),
                        )
                        .set("item_scheduled", tr(t, "Pattern scheduled."))
                        .set("item_updated", tr(t, "Pattern updated.")),
                )
                .set("public", false)
                .set("_builtin", true)
                .set("show_ui", true)
                .set("show_in_menu", false)
                .set("rewrite", false)
                .set("show_in_rest", true)
                .set("rest_base", "blocks")
                .set("rest_controller_class", "WP_REST_Blocks_Controller")
                .set("capability_type", "block")
                .set(
                    "capabilities",
                    pairs(&[
                        ("read", "edit_posts"),
                        ("create_posts", "publish_posts"),
                        ("edit_posts", "edit_posts"),
                        ("edit_published_posts", "edit_published_posts"),
                        ("delete_published_posts", "delete_published_posts"),
                        ("delete_posts", "delete_posts"),
                        ("edit_others_posts", "edit_others_posts"),
                        ("delete_others_posts", "delete_others_posts"),
                    ]),
                )
                .set("map_meta_cap", true)
                .set(
                    "supports",
                    list(&[
                        "title",
                        "excerpt",
                        "editor",
                        "revisions",
                        "custom-fields",
                        "author",
                    ]),
                )
                // Keep patterns when their author is deleted, as they may be used across the site.
                .set("delete_with_user", false)
                .items(),
        ),
        InitialCall::PostType(
            "wp_template",
            Arr::new()
                .set(
                    "labels",
                    Arr::new()
                        .set("name", x(t, "Templates", "post type general name"))
                        .set("singular_name", x(t, "Template", "post type singular name"))
                        .set("add_new", tr(t, "Add Template"))
                        .set("add_new_item", tr(t, "Add Template"))
                        .set("new_item", tr(t, "New Template"))
                        .set("edit_item", tr(t, "Edit Template"))
                        .set("view_item", tr(t, "View Template"))
                        .set("all_items", tr(t, "Templates"))
                        .set("search_items", tr(t, "Search Templates"))
                        .set("parent_item_colon", tr(t, "Parent Template:"))
                        .set("not_found", tr(t, "No templates found."))
                        .set("not_found_in_trash", tr(t, "No templates found in Trash."))
                        .set("archives", tr(t, "Template archives"))
                        .set("insert_into_item", tr(t, "Insert into template"))
                        .set("uploaded_to_this_item", tr(t, "Uploaded to this template"))
                        .set("filter_items_list", tr(t, "Filter templates list"))
                        .set("items_list_navigation", tr(t, "Templates list navigation"))
                        .set("items_list", tr(t, "Templates list"))
                        .set("item_updated", tr(t, "Template updated.")),
                )
                .set("description", tr(t, "Templates to include in your theme."))
                .set("public", false)
                .set("_builtin", true)
                .set("_edit_link", TEMPLATE_EDIT_LINK)
                .set("has_archive", false)
                .set("show_ui", false)
                .set("show_in_menu", false)
                .set("show_in_rest", true)
                .set("rewrite", false)
                .set("rest_base", "templates")
                .set("rest_controller_class", "WP_REST_Templates_Controller")
                .set(
                    "autosave_rest_controller_class",
                    "WP_REST_Template_Autosaves_Controller",
                )
                .set(
                    "revisions_rest_controller_class",
                    "WP_REST_Template_Revisions_Controller",
                )
                .set("late_route_registration", true)
                .set("capability_type", list(&["template", "templates"]))
                .set("capabilities", template_caps())
                .set("map_meta_cap", true)
                .set("supports", template_supports())
                .items(),
        ),
        InitialCall::PostType(
            "wp_template_part",
            Arr::new()
                .set(
                    "labels",
                    Arr::new()
                        .set("name", x(t, "Template Parts", "post type general name"))
                        .set(
                            "singular_name",
                            x(t, "Template Part", "post type singular name"),
                        )
                        .set("add_new", tr(t, "Add Template Part"))
                        .set("add_new_item", tr(t, "Add Template Part"))
                        .set("new_item", tr(t, "New Template Part"))
                        .set("edit_item", tr(t, "Edit Template Part"))
                        .set("view_item", tr(t, "View Template Part"))
                        .set("all_items", tr(t, "Template Parts"))
                        .set("search_items", tr(t, "Search Template Parts"))
                        .set("parent_item_colon", tr(t, "Parent Template Part:"))
                        .set("not_found", tr(t, "No template parts found."))
                        .set(
                            "not_found_in_trash",
                            tr(t, "No template parts found in Trash."),
                        )
                        .set("archives", tr(t, "Template part archives"))
                        .set("insert_into_item", tr(t, "Insert into template part"))
                        .set(
                            "uploaded_to_this_item",
                            tr(t, "Uploaded to this template part"),
                        )
                        .set("filter_items_list", tr(t, "Filter template parts list"))
                        .set(
                            "items_list_navigation",
                            tr(t, "Template parts list navigation"),
                        )
                        .set("items_list", tr(t, "Template parts list"))
                        .set("item_updated", tr(t, "Template part updated.")),
                )
                .set(
                    "description",
                    tr(t, "Template parts to include in your templates."),
                )
                .set("public", false)
                .set("_builtin", true)
                .set("_edit_link", TEMPLATE_EDIT_LINK)
                .set("has_archive", false)
                .set("show_ui", false)
                .set("show_in_menu", false)
                .set("show_in_rest", true)
                .set("rewrite", false)
                .set("rest_base", "template-parts")
                .set("rest_controller_class", "WP_REST_Templates_Controller")
                .set(
                    "autosave_rest_controller_class",
                    "WP_REST_Template_Autosaves_Controller",
                )
                .set(
                    "revisions_rest_controller_class",
                    "WP_REST_Template_Revisions_Controller",
                )
                .set("late_route_registration", true)
                .set("map_meta_cap", true)
                .set("capabilities", template_caps())
                .set("supports", template_supports())
                .items(),
        ),
        InitialCall::PostType(
            "wp_global_styles",
            Arr::new()
                .set("label", x(t, "Global Styles", "post type general name"))
                .set("description", tr(t, "Global styles to include in themes."))
                .set("public", false)
                .set("_builtin", true)
                .set("_edit_link", "/site-editor.php?canvas=edit")
                .set("show_ui", false)
                .set("show_in_rest", true)
                .set("rewrite", false)
                .set("rest_base", "global-styles")
                .set("rest_controller_class", "WP_REST_Global_Styles_Controller")
                .set(
                    "revisions_rest_controller_class",
                    "WP_REST_Global_Styles_Revisions_Controller",
                )
                .set("late_route_registration", true)
                .set(
                    "capabilities",
                    Arr::new()
                        .set("read", "edit_posts")
                        .set("create_posts", "edit_theme_options")
                        .set("edit_posts", "edit_theme_options")
                        .set("edit_published_posts", "edit_theme_options")
                        .set("delete_published_posts", "edit_theme_options")
                        .set("edit_others_posts", "edit_theme_options")
                        .set("delete_others_posts", "edit_theme_options"),
                )
                .set("map_meta_cap", true)
                .set("supports", list(&["title", "editor", "revisions"]))
                .items(),
        ),
        // Disable autosave endpoints for global styles.
        InitialCall::RemoveSupport("wp_global_styles", "autosave"),
        InitialCall::PostType(
            "wp_navigation",
            Arr::new()
                .set(
                    "labels",
                    Arr::new()
                        .set("name", x(t, "Navigation Menus", "post type general name"))
                        .set(
                            "singular_name",
                            x(t, "Navigation Menu", "post type singular name"),
                        )
                        .set("add_new", tr(t, "Add Navigation Menu"))
                        .set("add_new_item", tr(t, "Add Navigation Menu"))
                        .set("new_item", tr(t, "New Navigation Menu"))
                        .set("edit_item", tr(t, "Edit Navigation Menu"))
                        .set("view_item", tr(t, "View Navigation Menu"))
                        .set("all_items", tr(t, "Navigation Menus"))
                        .set("search_items", tr(t, "Search Navigation Menus"))
                        .set("parent_item_colon", tr(t, "Parent Navigation Menu:"))
                        .set("not_found", tr(t, "No Navigation Menu found."))
                        .set(
                            "not_found_in_trash",
                            tr(t, "No Navigation Menu found in Trash."),
                        )
                        .set("archives", tr(t, "Navigation Menu archives"))
                        .set("insert_into_item", tr(t, "Insert into Navigation Menu"))
                        .set(
                            "uploaded_to_this_item",
                            tr(t, "Uploaded to this Navigation Menu"),
                        )
                        .set("filter_items_list", tr(t, "Filter Navigation Menu list"))
                        .set(
                            "items_list_navigation",
                            tr(t, "Navigation Menus list navigation"),
                        )
                        .set("items_list", tr(t, "Navigation Menus list"))
                        .set("item_updated", tr(t, "Navigation Menu updated.")),
                )
                .set(
                    "description",
                    tr(t, "Navigation menus that can be inserted into your site."),
                )
                .set("public", false)
                .set("_builtin", true)
                .set("_edit_link", NAVIGATION_EDIT_LINK)
                .set("has_archive", false)
                .set("show_ui", true)
                .set("show_in_menu", false)
                .set("show_in_admin_bar", false)
                .set("show_in_rest", true)
                .set("rewrite", false)
                .set("map_meta_cap", true)
                .set(
                    "capabilities",
                    theme_options(&[
                        "edit_others_posts",
                        "delete_posts",
                        "publish_posts",
                        "create_posts",
                        "read_private_posts",
                        "delete_private_posts",
                        "delete_published_posts",
                        "delete_others_posts",
                        "edit_private_posts",
                        "edit_published_posts",
                        "edit_posts",
                    ]),
                )
                .set("rest_base", "navigation")
                .set("rest_controller_class", "WP_REST_Posts_Controller")
                .set("supports", list(&["title", "editor", "revisions"]))
                .items(),
        ),
        InitialCall::PostType(
            "wp_font_family",
            Arr::new()
                .set(
                    "labels",
                    labels(
                        t,
                        &[("name", "Font Families"), ("singular_name", "Font Family")],
                    ),
                )
                .set("public", false)
                .set("_builtin", true)
                .set("hierarchical", false)
                .set("capabilities", font_caps())
                .set("map_meta_cap", true)
                .set("query_var", false)
                .set("rewrite", false)
                .set("show_in_rest", true)
                .set("rest_base", "font-families")
                .set("rest_controller_class", "WP_REST_Font_Families_Controller")
                .set("supports", list(&["title"]))
                .items(),
        ),
        InitialCall::PostType(
            "wp_font_face",
            Arr::new()
                .set(
                    "labels",
                    labels(t, &[("name", "Font Faces"), ("singular_name", "Font Face")]),
                )
                .set("public", false)
                .set("_builtin", true)
                .set("hierarchical", false)
                .set("capabilities", font_caps())
                .set("map_meta_cap", true)
                .set("query_var", false)
                .set("rewrite", false)
                .set("show_in_rest", true)
                .set(
                    "rest_base",
                    "font-families/(?P<font_family_id>[\\d]+)/font-faces",
                )
                .set("rest_controller_class", "WP_REST_Font_Faces_Controller")
                .set("supports", list(&["title"]))
                .items(),
        ),
    ]
}

/// `label_count`'s text for a status: `"{label} <span class=\"count\">(%s)</span>"`.
fn counted(label: &str) -> String {
    format!("{label} <span class=\"count\">(%s)</span>")
}

fn status_steps(t: &dyn Translator) -> Vec<InitialCall> {
    // `register_post_status()` for the request statuses differs only in its label and count text.
    let request = |name: &'static str, label: &str| {
        InitialCall::Status(
            name,
            Arr::new()
                .set("label", x(t, label, "request status"))
                .set("internal", true)
                .set("_builtin", true)
                .set("label_count", count(&counted(label), &counted(label)))
                .set("exclude_from_search", false)
                .items(),
        )
    };
    vec![
        InitialCall::Status(
            "publish",
            Arr::new()
                .set("label", x(t, "Published", "post status"))
                .set("public", true)
                .set("_builtin", true)
                .set(
                    "label_count",
                    count(&counted("Published"), &counted("Published")),
                )
                .items(),
        ),
        InitialCall::Status(
            "future",
            Arr::new()
                .set("label", x(t, "Scheduled", "post status"))
                .set("protected", true)
                .set("_builtin", true)
                .set(
                    "label_count",
                    count(&counted("Scheduled"), &counted("Scheduled")),
                )
                .items(),
        ),
        InitialCall::Status(
            "draft",
            Arr::new()
                .set("label", x(t, "Draft", "post status"))
                .set("protected", true)
                .set("_builtin", true)
                .set("label_count", count(&counted("Draft"), &counted("Drafts")))
                .set("date_floating", true)
                .items(),
        ),
        InitialCall::Status(
            "pending",
            Arr::new()
                .set("label", x(t, "Pending", "post status"))
                .set("protected", true)
                .set("_builtin", true)
                .set(
                    "label_count",
                    count(&counted("Pending"), &counted("Pending")),
                )
                .set("date_floating", true)
                .items(),
        ),
        InitialCall::Status(
            "private",
            Arr::new()
                .set("label", x(t, "Private", "post status"))
                .set("private", true)
                .set("_builtin", true)
                .set(
                    "label_count",
                    count(&counted("Private"), &counted("Private")),
                )
                .items(),
        ),
        InitialCall::Status(
            "trash",
            Arr::new()
                .set("label", x(t, "Trash", "post status"))
                .set("internal", true)
                .set("_builtin", true)
                .set("label_count", count(&counted("Trash"), &counted("Trash")))
                .set("show_in_admin_status_list", true)
                .items(),
        ),
        InitialCall::Status(
            "auto-draft",
            Arr::new()
                .set("label", "auto-draft")
                .set("internal", true)
                .set("_builtin", true)
                .set("date_floating", true)
                .items(),
        ),
        InitialCall::Status(
            "inherit",
            Arr::new()
                .set("label", "inherit")
                .set("internal", true)
                .set("_builtin", true)
                .set("exclude_from_search", false)
                .items(),
        ),
        request("request-pending", "Pending"),
        request("request-confirmed", "Confirmed"),
        request("request-failed", "Failed"),
        request("request-completed", "Completed"),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::labels::Untranslated;
    use speedyspec_wp_posts_contracts::{PostStatuses, PostTypes, RewriteContext, TitleSanitizer};

    struct Plain;
    impl RewriteContext for Plain {
        fn rewrite_applies(&self) -> bool {
            false
        }
    }
    impl TitleSanitizer for Plain {
        fn sanitize_title_with_dashes(&self, title: &[u8]) -> Vec<u8> {
            title.to_vec()
        }
    }

    #[test]
    fn initial_registers_in_wordpress_order() {
        let mut types = PostTypeRegistry::new();
        let mut statuses = PostStatusRegistry::new();
        let ports = PostTypePorts {
            translator: &Untranslated,
            rewrite: &Plain,
            titles: &Plain,
        };
        let registrations = create_initial_post_types(
            &mut types,
            &mut statuses,
            &InitialOptions::default(),
            &ports,
        );
        let order = [
            "post",
            "page",
            "attachment",
            "revision",
            "nav_menu_item",
            "custom_css",
            "customize_changeset",
            "oembed_cache",
            "user_request",
            "wp_block",
            "wp_template",
            "wp_template_part",
            "wp_global_styles",
            "wp_navigation",
            "wp_font_family",
            "wp_font_face",
        ];
        assert_eq!(
            types
                .post_types()
                .iter()
                .map(|t| t.name().as_str().to_owned())
                .collect::<Vec<_>>(),
            order
        );
        assert_eq!(
            registrations
                .iter()
                .map(|r| r.post_type.name().as_str())
                .collect::<Vec<_>>(),
            order
        );
        assert_eq!(
            statuses
                .statuses()
                .iter()
                .map(|s| s.name().as_str().to_owned())
                .collect::<Vec<_>>(),
            [
                "publish",
                "future",
                "draft",
                "pending",
                "private",
                "trash",
                "auto-draft",
                "inherit",
                "request-pending",
                "request-confirmed",
                "request-failed",
                "request-completed",
            ]
        );
    }
}
