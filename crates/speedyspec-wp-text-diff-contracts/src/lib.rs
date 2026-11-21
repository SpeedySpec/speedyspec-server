//! Contracts for WordPress's text diff (`wp_text_diff()`).
//!
//! The values a diff is asked for and answers with, and the ports an implementation is built from:
//!
//! - [`SequenceDiffer`] turns two token sequences into [`DiffOp`]s (Strategy: Horde's engine for WordPress's output,
//!   any other algorithm for callers that only need a correct diff);
//! - [`LineHtmlFilter`] is the `process_text_diff_html` filter;
//! - [`WhitespaceNormalizer`] is `normalize_whitespace()`, which belongs to the text formatting port.
//!
//! Text is bytes throughout: PHP strings are bytes, and WordPress's diff never assumes an encoding except when it
//! splits words.

use std::borrow::Cow;
use std::ops::Range;

/// One edit operation over two token sequences, as Horde's `Text_Diff_Op_*` classes, by index range.
///
/// `Copy` ranges always have the same length on both sides. A `Change` has a non-empty range on both sides; a
/// `Delete` has only `orig`, an `Add` only `fin`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DiffOp {
    /// Tokens unchanged on both sides (`Text_Diff_Op_copy`).
    Copy {
        orig: Range<usize>,
        fin: Range<usize>,
    },
    /// Tokens only in the original (`Text_Diff_Op_delete`).
    Delete { orig: Range<usize> },
    /// Tokens only in the final text (`Text_Diff_Op_add`).
    Add { fin: Range<usize> },
    /// Original tokens replaced by final tokens (`Text_Diff_Op_change`).
    Change {
        orig: Range<usize>,
        fin: Range<usize>,
    },
}

/// Computes the edit operations that turn `orig` into `fin`.
///
/// Implementations must return operations that cover both sequences in order, without gaps, and that never put two
/// operations of the same kind next to each other.
pub trait SequenceDiffer {
    /// Diffs two token sequences.
    fn diff(&self, orig: &[&[u8]], fin: &[&[u8]]) -> Vec<DiffOp>;
}

/// Which kind of row a line is rendered into; the third argument of `process_text_diff_html`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowKind {
    Added,
    Deleted,
    Unchanged,
}

impl RowKind {
    /// The name WordPress passes to `process_text_diff_html`.
    pub fn as_str(self) -> &'static str {
        match self {
            RowKind::Added => "added",
            RowKind::Deleted => "deleted",
            RowKind::Unchanged => "unchanged",
        }
    }
}

/// The `process_text_diff_html` filter, applied once to each escaped line of an added, deleted or unchanged row.
///
/// Rows built from a changed pair are not filtered, as in WordPress. The escaped line is written straight into the
/// output, so a filter that changes nothing costs nothing.
pub trait LineHtmlFilter {
    /// Filters one line in place.
    ///
    /// `out[from..]` holds the escaped line (WordPress's `$processed_line`), and `raw` is the line as it was. To
    /// change it, truncate `out` to `from` and write the filtered line. A filter that WordPress would run must be
    /// asked for every row, also when it then changes nothing, so the filter's run count stays right.
    fn filter(&self, out: &mut Vec<u8>, from: usize, raw: &[u8], kind: RowKind);
}

/// A [`LineHtmlFilter`] that changes nothing, for Rust callers and tests.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoLineFilter;

impl LineHtmlFilter for NoLineFilter {
    fn filter(&self, _out: &mut Vec<u8>, _from: usize, _raw: &[u8], _kind: RowKind) {}
}

/// `normalize_whitespace()`: what `wp_text_diff()` does to both texts before diffing them.
pub trait WhitespaceNormalizer {
    /// Normalizes one text.
    fn normalize<'a>(&self, text: &'a [u8]) -> Cow<'a, [u8]>;
}

/// One title argument: its text as PHP prints it, and whether PHP finds it truthy.
///
/// The two are apart because WordPress tests a title with `if`/`empty()` but prints it with string interpolation:
/// `'title_left' => '0'` is falsy, yet it is printed when `title_right` is set.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Title {
    /// What `"{$title}"` gives.
    pub text: Vec<u8>,
    /// What `! empty( $title )` gives.
    pub truthy: bool,
}

impl Title {
    /// A title given as a PHP string: falsy when empty or `"0"`.
    pub fn new(text: impl Into<Vec<u8>>) -> Title {
        let text = text.into();
        let truthy = !(text.is_empty() || text == b"0");
        Title { text, truthy }
    }
}

/// The arguments of `wp_text_diff()` after `wp_parse_args()`, reduced to what the output depends on.
///
/// Titles are printed as given: WordPress does not escape them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TableOptions {
    /// `title`: a `<caption>` when truthy.
    pub title: Title,
    /// `title_left`: the left header cell when either side title is truthy (`th` when it is, `td` when not).
    pub title_left: Title,
    /// `title_right`: the right header cell, in split view only.
    pub title_right: Title,
    /// `show_split_view`: two columns when true, one when false.
    pub split_view: bool,
}

impl Default for TableOptions {
    /// WordPress's defaults: no titles, split view.
    fn default() -> TableOptions {
        TableOptions {
            title: Title::default(),
            title_left: Title::default(),
            title_right: Title::default(),
            split_view: true,
        }
    }
}

/// The hidden screen-reader labels of each cell, already translated (`__( 'Added:' )` and the others).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RowLabels {
    pub added: Vec<u8>,
    pub deleted: Vec<u8>,
    pub unchanged: Vec<u8>,
}

impl Default for RowLabels {
    /// The untranslated labels.
    fn default() -> RowLabels {
        RowLabels {
            added: b"Added:".to_vec(),
            deleted: b"Deleted:".to_vec(),
            unchanged: b"Unchanged:".to_vec(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn row_kinds_carry_wordpress_names() {
        assert_eq!(RowKind::Added.as_str(), "added");
        assert_eq!(RowKind::Deleted.as_str(), "deleted");
        assert_eq!(RowKind::Unchanged.as_str(), "unchanged");
    }

    #[test]
    fn no_line_filter_leaves_the_processed_line() {
        let mut out = b"<td>&lt;b&gt;".to_vec();
        NoLineFilter.filter(&mut out, 4, b"<b>", RowKind::Added);
        assert_eq!(out, b"<td>&lt;b&gt;");
    }

    #[test]
    fn string_titles_follow_php_truthiness() {
        assert!(!Title::new("").truthy);
        assert!(!Title::new("0").truthy);
        assert!(Title::new("00").truthy);
        assert!(Title::new(" ").truthy);
        assert_eq!(Title::new("0").text, b"0");
    }

    #[test]
    fn defaults_match_wp_text_diff() {
        let options = TableOptions::default();
        assert!(options.split_view);
        assert!(!options.title.truthy && !options.title_left.truthy && !options.title_right.truthy);
        assert_eq!(RowLabels::default().unchanged, b"Unchanged:");
    }
}
