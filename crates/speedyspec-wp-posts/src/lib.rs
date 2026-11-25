//! WordPress's post type and post status rules: the key rule for names, the post type labels and capabilities,
//! `WP_Post_Type`'s argument resolution, the post type registry with its supports and meta capabilities, and the
//! post status rules and registry, the filtered post type and status lists, and the exists, hierarchical and
//! viewable predicates, and `create_initial_post_types()`.

mod caps;
mod initial;
mod key;
mod labels;
mod list;
mod post_type;
mod queries;
mod registry;
mod status;

pub use caps::{meta_capabilities, post_type_capabilities};
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
pub use post_type::{PostTypePorts, ResolvedPostType, apply_filtered_labels, resolve_post_type};
pub use queries::{
    is_post_status_viewable, is_post_type_hierarchical, is_post_type_viewable,
    post_status_name_viewable, post_type_exists, post_type_name_viewable,
};
pub use registry::{
    ActionToAdd, ActionToRemove, PostTypeRegistration, PostTypeRegistry, PostTypeUnregistration,
    SupportToAdd, add_supports, unregistration,
};
pub use status::{
    PostStatusRegistry, page_statuses, post_statuses, privacy_statuses, resolve_post_status,
};
