//! WordPress's HTML API syntax pieces, ported from WordPress 7.1.5.
//!
//! - [`decode`], [`decode_attribute`], [`decode_text_node`] and [`read_character_reference`] are `WP_HTML_Decoder`.
//! - [`named_reference`] is the lookup `WP_Token_Map::read_token()` does over the HTML5 named character references,
//!   re-exported from `speedyspec-wp-token-map`, which holds the table.
//! - [`start_tag_attributes`] is how `WP_HTML_Tag_Processor` reads a start tag's attributes, as kses's
//!   `wp_kses_hair()` uses it.
//!
//! The rest of the HTML API (the Tag Processor and HTML Processor) is a later port; these pieces come first because
//! kses needs them.

mod attributes;
mod decoder;

pub use attributes::{TagAttribute, start_tag_attributes, start_tag_attributes_into};
pub use decoder::{
    DecodeContext, code_point_to_utf8, decode, decode_attribute, decode_text_node,
    read_character_reference,
};
pub use speedyspec_wp_token_map::{named_reference, named_reference_count};
