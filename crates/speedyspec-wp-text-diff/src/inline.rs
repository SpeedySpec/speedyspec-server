//! WordPress's inline word diff of one line against another (`WP_Text_Diff_Renderer_inline` over
//! `new Text_Diff( 'auto', array( array( $orig ), array( $final ) ) )`), as segments.
//!
//! The table renderer needs three strings from it: the whole inline diff (only its lengths, for the "too different"
//! ratio), the line without `<ins>` runs, and the line without `<del>` runs. WordPress gets the last two with
//! `preg_replace()` and the lengths with `preg_match_all()` and `strip_tags()`. Here the segments give all three
//! directly: escaped text never contains `<`, and NUL bytes are gone before splitting, so the results are the same.

use speedyspec_wp_text_diff_contracts::SequenceDiffer;

use crate::blocks::{BlockEdit, CONTEXT, blocks};
use crate::escape::escape_valid;
use crate::tokens::trim_newlines;
use crate::words::split_words;

/// A piece of an inline diff, escaped.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Segment {
    Text(Vec<u8>),
    Ins(Vec<u8>),
    Del(Vec<u8>),
}

/// The inline diff of one line against another.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InlineDiff {
    /// The lines are the same: WordPress renders nothing at all.
    Same,
    /// The segments; WordPress ends the rendering with `"\n"`.
    Changed(Vec<Segment>),
}

/// The line splits into words that are not valid UTF-8: `preg_split()` returned `false` and WordPress's engine
/// throws a `TypeError`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidUtf8;

/// The inline diff of `orig` against `fin`, words diffed with `differ`.
///
/// Words are escaped without checking UTF-8 again: [`split_words`] only returns valid text.
pub fn inline_diff(
    orig: &[u8],
    fin: &[u8],
    differ: &dyn SequenceDiffer,
) -> Result<InlineDiff, InvalidUtf8> {
    if trim_newlines(orig) == trim_newlines(fin) {
        return Ok(InlineDiff::Same);
    }
    let a = split_words(orig).ok_or(InvalidUtf8)?;
    let b = split_words(fin).ok_or(InvalidUtf8)?;
    let a = a.slices();
    let b = b.slices();
    let ops = differ.diff(&a, &b);
    let mut segments = Vec::new();
    let encode = |tokens: &[&[u8]]| {
        let mut out = Vec::new();
        for t in tokens {
            escape_valid(&mut out, &trim_newlines(t));
        }
        out
    };
    for block in blocks(&ops, CONTEXT, CONTEXT) {
        for edit in block {
            match edit {
                BlockEdit::Context(r) => segments.push(Segment::Text(encode(&a[r]))),
                BlockEdit::Added(r) => segments.push(Segment::Ins(encode(&b[r]))),
                BlockEdit::Deleted(r) => segments.push(Segment::Del(encode(&a[r]))),
                BlockEdit::Changed(ro, rf) => {
                    // The words-level `_changed()`: move leading spaces both sides share out in front.
                    let (o0, f0) = (trim_newlines(a[ro.start]), trim_newlines(b[rf.start]));
                    let mut k = 0;
                    while o0.get(k) == Some(&b' ') && f0.get(k) == Some(&b' ') {
                        k += 1;
                    }
                    if k > 0 {
                        segments.push(Segment::Text(vec![b' '; k]));
                    }
                    let enc = |first: &[u8], rest: &[&[u8]]| {
                        let mut out = Vec::new();
                        escape_valid(&mut out, first);
                        for t in rest {
                            escape_valid(&mut out, &trim_newlines(t));
                        }
                        out
                    };
                    segments.push(Segment::Del(enc(&o0[k..], &a[ro.start + 1..ro.end])));
                    segments.push(Segment::Ins(enc(&f0[k..], &b[rf.start + 1..rf.end])));
                }
            }
        }
    }
    Ok(InlineDiff::Changed(segments))
}

impl InlineDiff {
    /// The rendering as WordPress returns it.
    pub fn render(&self) -> Vec<u8> {
        self.render_with(true, true)
    }

    /// The rendering without `<ins>` runs (`preg_replace( '|<ins>.*?</ins>|', '', $diff )`).
    pub fn orig_line(&self) -> Vec<u8> {
        self.render_with(false, true)
    }

    /// The rendering without `<del>` runs.
    pub fn final_line(&self) -> Vec<u8> {
        self.render_with(true, false)
    }

    fn render_with(&self, ins: bool, del: bool) -> Vec<u8> {
        let InlineDiff::Changed(segments) = self else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for s in segments {
            match s {
                Segment::Text(t) => out.extend_from_slice(t),
                Segment::Ins(t) if ins => {
                    out.extend_from_slice(b"<ins>");
                    out.extend_from_slice(t);
                    out.extend_from_slice(b"</ins>");
                }
                Segment::Del(t) if del => {
                    out.extend_from_slice(b"<del>");
                    out.extend_from_slice(t);
                    out.extend_from_slice(b"</del>");
                }
                _ => {}
            }
        }
        out.push(b'\n');
        out
    }

    /// The table renderer's ratio terms: `strlen( strip_tags( implode( ' ', $matches ) ) )` and
    /// `strlen( strip_tags( $diff ) )`, or `None` when there are no `<ins>` or `<del>` runs.
    pub fn ratio_terms(&self) -> Option<(i64, i64)> {
        let InlineDiff::Changed(segments) = self else {
            return None;
        };
        let mut matches = 0i64;
        let mut matched = 0i64;
        let mut total = 1i64; // the trailing "\n"
        for s in segments {
            match s {
                Segment::Text(t) => total += t.len() as i64,
                Segment::Ins(t) | Segment::Del(t) => {
                    matches += 1;
                    matched += t.len() as i64;
                    total += t.len() as i64;
                }
            }
        }
        (matches > 0).then_some((matched + matches - 1, total))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::horde::HordeSequenceDiffer;

    #[test]
    fn inserted_word() {
        let d = inline_diff(b"Hello world", b"Hello there world", &HordeSequenceDiffer).unwrap();
        assert_eq!(d.render(), b"Hello<ins> there</ins> world\n");
        assert_eq!(d.orig_line(), b"Hello world\n");
        assert_eq!(d.final_line(), b"Hello<ins> there</ins> world\n");
    }

    #[test]
    fn same_lines_render_nothing() {
        let d = inline_diff(b"a b", b"a b", &HordeSequenceDiffer).unwrap();
        assert_eq!(d, InlineDiff::Same);
        assert!(d.render().is_empty());
    }

    #[test]
    fn invalid_utf8_is_an_error() {
        assert_eq!(
            inline_diff(b"a\xff", b"b", &HordeSequenceDiffer),
            Err(InvalidUtf8)
        );
    }
}
