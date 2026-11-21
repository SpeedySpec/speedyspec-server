//! `wp_text_diff()`.

use std::borrow::Cow;

use speedyspec_wp_text_diff_contracts::{
    LineHtmlFilter, RowLabels, SequenceDiffer, TableOptions, WhitespaceNormalizer,
};

use crate::horde::HordeSequenceDiffer;
use crate::table::{RenderError, TableRenderer};

/// A [`WhitespaceNormalizer`] for text that is normalized already (the PHP facade normalizes it in PHP).
#[derive(Clone, Copy, Debug, Default)]
pub struct AlreadyNormalized;

impl WhitespaceNormalizer for AlreadyNormalized {
    fn normalize<'a>(&self, text: &'a [u8]) -> Cow<'a, [u8]> {
        Cow::Borrowed(text)
    }
}

/// Why `wp_text_diff()` failed where WordPress throws.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TextDiffError {
    /// A changed pair of lines is not valid UTF-8; WordPress throws a `TypeError`.
    InvalidUtf8,
    /// WordPress's "too different" ratio divided by zero; it throws a `DivisionByZeroError`.
    DivisionByZero,
    /// A changed block of `orig` × `fin` lines is too large to pair; WordPress runs out of memory there.
    TooLarge { orig: usize, fin: usize },
}

impl std::fmt::Display for TextDiffError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TextDiffError::InvalidUtf8 => {
                f.write_str("a changed line is not valid UTF-8, so it cannot be split into words")
            }
            TextDiffError::DivisionByZero => f.write_str("Division by zero"),
            TextDiffError::TooLarge { orig, fin } => write!(
                f,
                "a changed block of {orig} lines against {fin} lines is too large to pair line by line"
            ),
        }
    }
}

impl std::error::Error for TextDiffError {}

impl From<RenderError> for TextDiffError {
    fn from(e: RenderError) -> TextDiffError {
        match e {
            RenderError::InvalidUtf8 => TextDiffError::InvalidUtf8,
            RenderError::DivisionByZero => TextDiffError::DivisionByZero,
            RenderError::TooLarge { orig, fin } => TextDiffError::TooLarge { orig, fin },
        }
    }
}

/// WordPress's text diff: `wp_text_diff()` over a [`SequenceDiffer`] and a [`WhitespaceNormalizer`].
pub struct TextDiff<D = HordeSequenceDiffer, N = AlreadyNormalized> {
    differ: D,
    normalizer: N,
}

impl Default for TextDiff {
    fn default() -> TextDiff {
        TextDiff::new(HordeSequenceDiffer, AlreadyNormalized)
    }
}

impl<D: SequenceDiffer, N: WhitespaceNormalizer> TextDiff<D, N> {
    /// A text diff with this engine and normalizer.
    pub fn new(differ: D, normalizer: N) -> TextDiff<D, N> {
        TextDiff { differ, normalizer }
    }

    /// The table `wp_text_diff( $left, $right, $args )` returns, or `None` where it returns `''`.
    pub fn table(
        &self,
        left: &[u8],
        right: &[u8],
        options: &TableOptions,
        labels: &RowLabels,
        filter: &dyn LineHtmlFilter,
    ) -> Result<Option<Vec<u8>>, TextDiffError> {
        let left = self.normalizer.normalize(left);
        let right = self.normalizer.normalize(right);
        let left_lines: Vec<&[u8]> = split_lines(&left);
        let right_lines: Vec<&[u8]> = split_lines(&right);
        let ops = self.differ.diff(&left_lines, &right_lines);
        let renderer = TableRenderer {
            split_view: options.split_view,
            labels,
            filter,
            differ: &self.differ,
        };
        let rows = renderer.render(&left_lines, &right_lines, &ops)?;
        if rows.is_empty() {
            return Ok(None);
        }
        Ok(Some(wrap(&rows, options)))
    }
}

/// `explode( "\n", $s )`.
pub fn split_lines(s: &[u8]) -> Vec<&[u8]> {
    let mut lines = Vec::with_capacity(memchr::memchr_iter(b'\n', s).count() + 1);
    let mut start = 0;
    for i in memchr::memchr_iter(b'\n', s) {
        lines.push(&s[start..i]);
        start = i + 1;
    }
    lines.push(&s[start..]);
    lines
}

/// The `<table>` around the rows.
fn wrap(rows: &[u8], options: &TableOptions) -> Vec<u8> {
    let mut r = Vec::with_capacity(rows.len() + 256);
    r.extend_from_slice(b"<table class='diff");
    if options.split_view {
        r.extend_from_slice(b" is-split-view");
    }
    r.extend_from_slice(b"'>\n");
    if options.title.truthy {
        r.extend_from_slice(b"<caption class='diff-title'>");
        r.extend_from_slice(&options.title.text);
        r.extend_from_slice(b"</caption>\n");
    }
    if options.title_left.truthy || options.title_right.truthy {
        let left = if options.title_left.truthy {
            b"th"
        } else {
            b"td"
        };
        let right = if options.title_right.truthy {
            b"th"
        } else {
            b"td"
        };
        r.extend_from_slice(b"<thead><tr class='diff-sub-title'>\n");
        cell(&mut r, left, &options.title_left.text);
        if options.split_view {
            cell(&mut r, right, &options.title_right.text);
        }
        r.extend_from_slice(b"</tr>\n</thead>\n");
    }
    r.extend_from_slice(b"<tbody>\n");
    r.extend_from_slice(rows);
    r.extend_from_slice(b"\n</tbody>\n</table>");
    r
}

fn cell(r: &mut Vec<u8>, tag: &[u8], text: &[u8]) {
    r.extend_from_slice(b"\t<");
    r.extend_from_slice(tag);
    r.push(b'>');
    r.extend_from_slice(text);
    r.extend_from_slice(b"</");
    r.extend_from_slice(tag);
    r.extend_from_slice(b">\n");
}

#[cfg(test)]
mod tests {
    use super::*;
    use speedyspec_wp_text_diff_contracts::{NoLineFilter, Title};

    #[test]
    fn identical_text_is_no_table() {
        let t = TextDiff::default().table(
            b"same",
            b"same",
            &TableOptions::default(),
            &RowLabels::default(),
            &NoLineFilter,
        );
        assert_eq!(t, Ok(None));
    }

    #[test]
    fn caption_only_when_truthy() {
        let options = TableOptions {
            title: Title::new("0"),
            ..TableOptions::default()
        };
        let t = TextDiff::default()
            .table(b"a", b"b", &options, &RowLabels::default(), &NoLineFilter)
            .unwrap()
            .unwrap();
        assert!(!t.windows(8).any(|w| w == b"<caption"));
    }

    #[test]
    fn a_huge_changed_block_fails_instead_of_exhausting_memory() {
        let left: Vec<u8> = (0..2100)
            .flat_map(|i| format!("left {i}\n").into_bytes())
            .collect();
        let right: Vec<u8> = (0..2100)
            .flat_map(|i| format!("RIGHT {i}\n").into_bytes())
            .collect();
        let t = TextDiff::default().table(
            &left,
            &right,
            &TableOptions::default(),
            &RowLabels::default(),
            &NoLineFilter,
        );
        assert!(matches!(t, Err(TextDiffError::TooLarge { .. })), "{t:?}");
    }

    #[test]
    fn splits_like_explode() {
        assert_eq!(split_lines(b""), [b"".as_slice()]);
        assert_eq!(split_lines(b"a\n\nb\n"), [b"a".as_slice(), b"", b"b", b""]);
    }
}
