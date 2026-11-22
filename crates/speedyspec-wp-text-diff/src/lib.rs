//! WordPress's `wp_text_diff()` in Rust.
//!
//! - [`TextDiff`] is the function: lines diffed by a [`SequenceDiffer`], rows rendered as
//!   `WP_Text_Diff_Renderer_Table` renders them, the table around them.
//! - [`HordeSequenceDiffer`] is Horde's `Text_Diff_Engine_native`, ported line for line, so the output is
//!   WordPress's byte for byte.
//! - The string work (escaping, word splitting, line splitting, byte counts) runs 16 bytes at a time where the CPU
//!   allows ([`simd`]), with scalar twins the vector code is tested against.
//!
//! Text is bytes throughout, as in PHP. Input is expected to be normalized (`normalize_whitespace()`) by the
//! [`WhitespaceNormalizer`] given to [`TextDiff::new`].
//!
//! [`SequenceDiffer`]: speedyspec_wp_text_diff_contracts::SequenceDiffer
//! [`WhitespaceNormalizer`]: speedyspec_wp_text_diff_contracts::WhitespaceNormalizer

mod api;
mod blocks;
mod escape;
mod histogram;
mod horde;
mod inline;
mod numeric;
mod port;
pub mod simd;
mod table;
mod tokens;
mod whitespace;
mod words;

pub use api::{AlreadyNormalized, TextDiff, TextDiffError, split_lines};
pub use escape::{escape_into, html_special_chars};
pub use histogram::{byte_counts, l1_distance, string_distance};
pub use horde::HordeSequenceDiffer;
pub use inline::{InlineDiff, InvalidUtf8, Segment, inline_diff};
pub use numeric::{Numeric, parse_numeric, php_loose_eq};
pub use port::{WordPressTextDiffer, php_truthy};
pub use speedyspec_wp_text_diff_contracts as contracts;
pub use table::MAX_PAIRS;
pub use whitespace::{WordPressWhitespace, normalize_whitespace};
pub use words::{Words, is_word_char, split_words};
