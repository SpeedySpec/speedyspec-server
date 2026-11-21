//! WordPress's text formatting functions, in Rust.
//!
//! The content filters (`wptexturize`, `wpautop` and the rest) as pure functions over bytes, matching stock
//! WordPress byte for byte, and a `TextApi` facade that fires WordPress's filters around them.

pub mod preg;
pub mod strings;

mod ent2ncr_table;
mod small;
mod strip;
mod trim;

pub use small::{
    DEFAULT_SPACES, capital_p_dangit, convert_chars, convert_invalid_entities, ent2ncr, format_for_editor_core,
    normalize_whitespace,
};
pub use strip::{strip_tags, wp_strip_all_tags_core};
pub use trim::{wp_html_excerpt, wp_trim_words_core};
