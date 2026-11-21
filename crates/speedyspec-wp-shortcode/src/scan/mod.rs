//! The [`ShortcodeScanner`](speedyspec_wp_shortcode_contracts::ShortcodeScanner) strategies.

mod fancy;
mod hand;
mod tokens;

pub use fancy::FancyRegexScanner;
pub use hand::HandScanner;
pub use tokens::candidate_tokens;
