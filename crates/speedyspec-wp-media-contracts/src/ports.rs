//! The ports WordPress's media rules reach other WordPress code through.

use crate::Tree;

/// WordPress's `wp_kses_post()` and `wp_kses_post_deep()`, which the rules run tag values through. Bytes in, bytes
/// out: a tag that isn't UTF-8 (a Latin-1 ID3v1 title) reaches kses as WordPress would pass it.
pub trait TagSanitizer {
    fn post(&self, value: &[u8]) -> Vec<u8>;
    fn post_deep(&self, value: &Tree) -> Tree;
}

/// PHP's `strtotime()`, which WordPress uses for Matroska's creation time; `None` where PHP returns `false`.
pub trait TimeParser {
    fn strtotime(&self, text: &[u8]) -> Option<i64>;
}

/// The MIME type of a file's content, as PHP's fileinfo (`FILEINFO_MIME_TYPE`) names it; `None` when it can't tell.
/// `wp_check_filetype_and_ext()` compares it with the type the file's name claims.
pub trait ContentSniffer {
    fn sniff(&self, path: &std::path::Path) -> Option<String>;
}
