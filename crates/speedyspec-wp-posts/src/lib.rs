//! WordPress's post type and post status rules: the key rule for names and the post type labels.

mod key;
mod labels;

pub use key::{NAME_LENGTH_VERSION, check_post_type_name, sanitize_key};
pub use labels::{
    CustomLabels, LabelPair, LabelSource, Untranslated, custom_object_labels, default_labels,
    merge_filtered_labels, post_type_labels,
};
