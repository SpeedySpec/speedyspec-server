//! The [`AttsParser`](speedyspec_wp_shortcode_contracts::AttsParser) strategies for `shortcode_parse_atts()`.

mod hand;
mod regex;
mod shared;

pub use hand::HandAttsParser;
pub use regex::RegexAttsParser;

pub(crate) use shared::is_space as is_pcre_space;
