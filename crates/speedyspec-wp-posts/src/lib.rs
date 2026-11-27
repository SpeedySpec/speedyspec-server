//! WordPress's post type and post status rules: the key rule for names, the post type labels and capabilities,
//! `WP_Post_Type`'s argument resolution, the post type registry with its supports and meta capabilities, and the
//! post status rules and registry, the filtered post type and status lists, and the exists, hierarchical and
//! viewable predicates, `create_initial_post_types()`, `wp_unique_post_slug()`'s rules over the post repository,
//! the post hierarchy: parents, loops, ancestors, page URIs and page trees, `get_extended()`'s more split, and
//! `sanitize_post()` and `sanitize_post_field()` on PHP-shaped values over the [`Formatting`] port; the post type
//! labels, viewable and unique slug filters run through the hooks contract.

mod caps;
mod content;
mod dates;
mod hierarchy;
mod hook;
mod initial;
mod key;
mod labels;
mod list;
mod pages;
mod post_type;
mod queries;
mod reader;
mod registry;
mod sanitize;
mod slug;
mod status;
#[cfg(test)]
mod test_support;

pub use caps::{meta_capabilities, post_type_capabilities};
pub use content::{Extended, extended};
pub use dates::{
    InsertDateInput, InsertDates, ResolvedPostDate, gregorian_date_valid, insert_post_dates,
    resolve_post_date,
};
pub use hierarchy::{
    ParentCheck, check_post_hierarchy_for_loops, find_hierarchy_loop, page_uri, post_ancestors,
    post_parent_id,
};
pub use initial::{InitialOptions, create_initial_post_types};

pub use key::{NAME_LENGTH_VERSION, check_post_type_name, sanitize_key};
pub use labels::{
    CustomLabels, LabelPair, LabelSource, Untranslated, custom_object_labels, default_labels,
    merge_filtered_labels, post_type_labels,
};
pub use list::{
    FilterOperator, ListItem, filter_object_list, post_stati, post_status_names, post_type_names,
    post_types, post_types_by_support,
};
pub use pages::{page_children, page_hierarchy};
pub use post_type::{
    PostTypePorts, ResolvedPostType, apply_filtered_labels, filter_post_type_labels,
    resolve_post_type,
};
pub use queries::{
    is_post_status_viewable, is_post_status_viewable_filtered, is_post_type_hierarchical,
    is_post_type_viewable, is_post_type_viewable_filtered, post_status_name_viewable,
    post_status_name_viewable_filtered, post_type_exists, post_type_name_viewable,
    post_type_name_viewable_filtered,
};
pub use reader::Uncached;
pub use registry::{
    ActionToAdd, ActionToRemove, PostTypeRegistration, PostTypeRegistry, PostTypeUnregistration,
    SupportToAdd, add_supports, unregistration,
};
pub use sanitize::{
    Formatting, SanitizePorts, sanitize_post_field, sanitize_post_field_word,
    sanitize_post_properties, sanitize_post_value,
};
pub use slug::{
    NoBadSlugs, SlugContext, SlugPorts, SlugRequest, slug_needs_no_check, unique_post_slug,
    unique_post_slug_filtered,
};
pub use status::{
    PostStatusRegistry, page_statuses, post_statuses, privacy_statuses, resolve_post_status,
};
