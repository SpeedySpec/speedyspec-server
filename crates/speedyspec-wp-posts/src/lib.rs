//! WordPress's post type and post status rules: the key rule for names, the post type labels and capabilities,
//! `WP_Post_Type`'s argument resolution, the post type registry with its supports and meta capabilities, and the
//! post status rules and registry.

mod caps;
mod key;
mod labels;
mod post_type;
mod registry;
mod status;

pub use caps::{meta_capabilities, post_type_capabilities};

pub use key::{NAME_LENGTH_VERSION, check_post_type_name, sanitize_key};
pub use labels::{
    CustomLabels, LabelPair, LabelSource, Untranslated, custom_object_labels, default_labels,
    merge_filtered_labels, post_type_labels,
};
pub use post_type::{PostTypePorts, ResolvedPostType, apply_filtered_labels, resolve_post_type};
pub use registry::{
    ActionToAdd, ActionToRemove, PostTypeRegistration, PostTypeRegistry, PostTypeUnregistration,
};
pub use status::{
    PostStatusRegistry, page_statuses, post_statuses, privacy_statuses, resolve_post_status,
};
