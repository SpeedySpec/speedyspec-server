//! `speedyspec_ci`: MySQL's case- and accent-insensitive comparison with PAD SPACE, as `utf8mb4_unicode_520_ci`
//! compares.

use unicode_normalization::UnicodeNormalization;
use unicode_normalization::char::is_combining_mark;

/// The form two strings are compared in: decomposed, combining marks removed, lowercase, trailing spaces trimmed.
pub fn fold(s: &str) -> String {
    let folded: String = s
        .nfd()
        .filter(|c| !is_combining_mark(*c))
        .collect::<String>()
        .to_lowercase();
    folded.trim_end_matches(' ').to_string()
}

pub(crate) fn register(conn: &rusqlite::Connection) -> rusqlite::Result<()> {
    conn.create_collation("speedyspec_ci", |a: &str, b: &str| fold(a).cmp(&fold(b)))
}
