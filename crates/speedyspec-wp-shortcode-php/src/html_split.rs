//! The `HtmlSplitter` port onto the Rust `wp_html_split()` (Adapter).

use speedyspec_wp_shortcode_contracts::HtmlSplitter;

/// `wp_html_split()` through `speedyspec-wp-html-split`, which matches `get_html_split_regex()` byte for byte.
///
/// It never returns `preg_split()`'s `false` on backtrack exhaustion, which is the Shortcode API's own deliberate
/// difference from WordPress too.
#[derive(Clone, Copy, Debug, Default)]
pub struct RustHtmlSplitter;

impl HtmlSplitter for RustHtmlSplitter {
    /// Splits `content` into alternating text and element chunks, text first and last, as slices of it.
    fn split<'a>(&self, content: &'a [u8]) -> Vec<&'a [u8]> {
        speedyspec_wp_html_split::wp_html_split(content)
    }
}
