//! WordPress's text formatting functions, in Rust.
//!
//! The content filters (`wptexturize`, `wpautop` and the rest) as pure functions over bytes, matching stock
//! WordPress byte for byte, and a `TextApi` facade that fires WordPress's filters around them.

pub mod deep;
pub mod email;
pub mod esc_url;
pub mod file_name;
pub mod helpers;
pub mod links;
pub mod messages;
pub mod parse_str;
pub mod preg;
pub mod sanitize;
pub mod smilies;
pub mod strings;
pub mod title;
pub mod url;

mod accents;
mod accents_table;
pub mod adapters;
mod api;
mod api_links;
mod api_sanitize;
mod api_smilies;
mod autop;
mod autop_fast;
mod balance;
mod ent2ncr_table;
mod mb_lower_table;
mod register;
mod scan;
mod small;
mod strip;
mod texturize;
mod trim;
mod unautop;

pub use small::{
    DEFAULT_SPACES, capital_p_dangit, convert_chars, convert_invalid_entities, ent2ncr, format_for_editor_core,
    normalize_whitespace,
};
pub use strip::{strip_tags, wp_strip_all_tags_core};
pub use trim::{wp_html_excerpt, wp_trim_words_core};
pub use autop::{AutopOptions, wp_replace_in_html_tags, wpautop_reference};
pub use unautop::{Unautop, shortcode_unautop};
pub use balance::force_balance_tags;
pub use texturize::{HandTexturizer, PcreTexturizer, TexturizeScope, TexturizeTables, Texturizer, texturize_shortcode_regex};
pub use api::{AutopFn, TextApi, TextDeps, text_error};
pub use register::{FilterContext, default_callback_names, hook_error as text_hook_error, register_text_filters};
pub use messages::register_text_handlers;
pub use autop_fast::wpautop;
