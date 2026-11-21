//! `WP_Text_Diff_Renderer_Table`: the rows of `wp_text_diff()`'s table.

use speedyspec_wp_text_diff_contracts::{LineHtmlFilter, RowKind, RowLabels, SequenceDiffer};

use crate::blocks::{BlockEdit, CONTEXT, blocks};
use crate::escape::escape_into;
use crate::histogram::{byte_counts, string_distance};
use crate::inline::{InlineDiff, InvalidUtf8, inline_diff};
use crate::tokens::trim_newlines;

/// Why rendering stopped: WordPress throws here.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RenderError {
    /// A paired changed line splits into words that are not valid UTF-8 (`TypeError` in WordPress).
    InvalidUtf8,
    /// The "too different" ratio divided by zero (`DivisionByZeroError` in WordPress).
    DivisionByZero,
}

impl From<InvalidUtf8> for RenderError {
    fn from(_: InvalidUtf8) -> RenderError {
        RenderError::InvalidUtf8
    }
}

/// The threshold above which a changed pair shows no `<ins>`/`<del>` (`$_diff_threshold`).
const DIFF_THRESHOLD: f64 = 0.6;

/// Renders the rows for one diff.
pub struct TableRenderer<'a> {
    pub split_view: bool,
    pub labels: &'a RowLabels,
    pub filter: &'a dyn LineHtmlFilter,
    pub differ: &'a dyn SequenceDiffer,
}

impl TableRenderer<'_> {
    /// The rows for `ops` over `orig` and `fin` (`render()`), empty when nothing changed.
    pub fn render(
        &self,
        orig: &[&[u8]],
        fin: &[&[u8]],
        ops: &[speedyspec_wp_text_diff_contracts::DiffOp],
    ) -> Result<Vec<u8>, RenderError> {
        let mut out = Vec::with_capacity((orig.len() + fin.len()) * 160);
        for block in blocks(ops, CONTEXT, CONTEXT) {
            for edit in block {
                match edit {
                    BlockEdit::Context(r) => self.context_rows(&mut out, &orig[r]),
                    BlockEdit::Added(r) => {
                        for line in &fin[r] {
                            self.added_row(&mut out, Content::Encode(&trim_newlines(line)));
                        }
                    }
                    BlockEdit::Deleted(r) => {
                        for line in &orig[r] {
                            self.deleted_row(&mut out, Content::Encode(&trim_newlines(line)));
                        }
                    }
                    BlockEdit::Changed(ro, rf) => {
                        let o: Vec<_> = orig[ro].iter().map(|l| trim_newlines(l)).collect();
                        let f: Vec<_> = fin[rf].iter().map(|l| trim_newlines(l)).collect();
                        let o: Vec<&[u8]> = o.iter().map(|l| l.as_ref()).collect();
                        let f: Vec<&[u8]> = f.iter().map(|l| l.as_ref()).collect();
                        self.changed(&mut out, &o, &f)?;
                    }
                }
            }
        }
        Ok(out)
    }

    fn open_added(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(b"<td class='diff-addedline'><span aria-hidden='true' class='dashicons dashicons-plus'></span><span class='screen-reader-text'>");
        out.extend_from_slice(&self.labels.added);
        out.extend_from_slice(b" </span>");
    }

    fn open_deleted(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(b"<td class='diff-deletedline'><span aria-hidden='true' class='dashicons dashicons-minus'></span><span class='screen-reader-text'>");
        out.extend_from_slice(&self.labels.deleted);
        out.extend_from_slice(b" </span>");
    }

    fn open_context(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(b"<td class='diff-context'><span class='screen-reader-text'>");
        out.extend_from_slice(&self.labels.unchanged);
        out.extend_from_slice(b" </span>");
    }

    const EMPTY: &'static [u8] = b"<td>&nbsp;</td>";

    /// Writes a cell's content: escaped and filtered, or given.
    fn content(&self, out: &mut Vec<u8>, content: Content<'_>, kind: RowKind) {
        match content {
            Content::Encode(line) => {
                let from = out.len();
                escape_into(out, line);
                self.filter.filter(out, from, line, kind);
            }
            Content::Html(html) => out.extend_from_slice(html),
        }
    }

    fn added_row(&self, out: &mut Vec<u8>, content: Content<'_>) {
        out.extend_from_slice(b"<tr>");
        if self.split_view {
            out.extend_from_slice(Self::EMPTY);
        }
        self.open_added(out);
        self.content(out, content, RowKind::Added);
        out.extend_from_slice(b"</td></tr>\n");
    }

    fn deleted_row(&self, out: &mut Vec<u8>, content: Content<'_>) {
        out.extend_from_slice(b"<tr>");
        self.open_deleted(out);
        self.content(out, content, RowKind::Deleted);
        out.extend_from_slice(b"</td>");
        if self.split_view {
            out.extend_from_slice(Self::EMPTY);
        }
        out.extend_from_slice(b"</tr>\n");
    }

    fn context_rows(&self, out: &mut Vec<u8>, lines: &[&[u8]]) {
        for line in lines {
            let line = trim_newlines(line);
            out.extend_from_slice(b"<tr>");
            self.open_context(out);
            let from = out.len();
            self.content(out, Content::Encode(&line), RowKind::Unchanged);
            let to = out.len();
            out.extend_from_slice(b"</td>");
            if self.split_view {
                self.open_context(out);
                out.extend_from_within(from..to);
                out.extend_from_slice(b"</td>");
            }
            out.extend_from_slice(b"</tr>\n");
        }
    }

    /// `_changed()`: pairs the lines, word-diffs each pair, and writes the rows with blank padding.
    fn changed(&self, out: &mut Vec<u8>, orig: &[&[u8]], fin: &[&[u8]]) -> Result<(), RenderError> {
        let (orig_matches, orig_rows, final_rows) = interleave(orig, fin);

        let mut orig_diffs: Vec<Option<Vec<u8>>> = vec![None; orig.len()];
        let mut final_diffs: Vec<Option<Vec<u8>>> = vec![None; fin.len()];
        for (o, m) in orig_matches.iter().enumerate() {
            let Some(f) = *m else { continue };
            let diff = inline_diff(orig[o], fin[f], self.differ)?;
            if let Some((matched, total)) = diff.ratio_terms() {
                let stripped_diff = total * 2 - matched;
                if stripped_diff == 0 {
                    return Err(RenderError::DivisionByZero);
                }
                if matched as f64 / stripped_diff as f64 > DIFF_THRESHOLD {
                    continue;
                }
            }
            match &diff {
                InlineDiff::Same => {
                    orig_diffs[o] = Some(Vec::new());
                    final_diffs[f] = Some(Vec::new());
                }
                InlineDiff::Changed(_) => {
                    orig_diffs[o] = Some(diff.orig_line());
                    final_diffs[f] = Some(diff.final_line());
                }
            }
        }

        let mut line_buf = Vec::new();
        for (row, &orow) in orig_rows.iter().enumerate() {
            let frow = final_rows[row];
            if orow < 0 && frow < 0 {
                continue;
            }
            let orig_line: Vec<u8> = cell_line(orow, &orig_diffs, orig, &mut line_buf);
            let final_line: Vec<u8> = cell_line(frow, &final_diffs, fin, &mut line_buf);
            if orow < 0 {
                self.added_row(out, Content::Html(&final_line));
            } else if frow < 0 {
                self.deleted_row(out, Content::Html(&orig_line));
            } else {
                out.extend_from_slice(b"<tr>");
                self.open_deleted(out);
                out.extend_from_slice(&orig_line);
                out.extend_from_slice(b"</td>");
                if !self.split_view {
                    out.extend_from_slice(b"</tr><tr>");
                }
                self.open_added(out);
                out.extend_from_slice(&final_line);
                out.extend_from_slice(b"</td></tr>\n");
            }
        }
        Ok(())
    }
}

/// A cell's content.
enum Content<'a> {
    /// A line to escape and filter.
    Encode(&'a [u8]),
    /// HTML written as is.
    Html(&'a [u8]),
}

/// The word diff when there is one, else the escaped line, else nothing (a padding row).
fn cell_line(row: i64, diffs: &[Option<Vec<u8>>], lines: &[&[u8]], buf: &mut Vec<u8>) -> Vec<u8> {
    if row < 0 {
        return Vec::new();
    }
    let i = row as usize;
    if let Some(d) = &diffs[i] {
        return d.clone();
    }
    buf.clear();
    escape_into(buf, lines[i]);
    buf.clone()
}

/// `interleave_changed_lines()`: pairs each original line with its closest final line, then pads both columns with
/// blank rows (negative numbers) so paired lines sit side by side.
///
/// Returns each original line's match (`None` for WordPress's `'x'`), and both columns' rows.
pub fn interleave(orig: &[&[u8]], fin: &[&[u8]]) -> (Vec<Option<usize>>, Vec<i64>, Vec<i64>) {
    let counts_o: Vec<[u32; 256]> = orig.iter().map(|l| byte_counts(l)).collect();
    let counts_f: Vec<[u32; 256]> = fin.iter().map(|l| byte_counts(l)).collect();
    let mut matches = Vec::with_capacity(orig.len() * fin.len());
    for (o, (line, counts)) in orig.iter().zip(&counts_o).enumerate() {
        for (f, counts_final) in counts_f.iter().enumerate() {
            matches.push((string_distance(line, counts, counts_final), o, f));
        }
    }
    // asort(): stable since PHP 8.0, so ties keep insertion order.
    matches.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));

    #[derive(Clone, Copy, PartialEq)]
    enum M {
        Unset,
        To(usize),
        X,
    }
    let mut orig_matches = vec![M::Unset; orig.len()];
    let mut final_matches = vec![M::Unset; fin.len()];
    for &(_, o, f) in &matches {
        let (os, fs) = (orig_matches[o] != M::Unset, final_matches[f] != M::Unset);
        if os && fs {
            continue;
        }
        if !os && !fs {
            orig_matches[o] = M::To(f);
            final_matches[f] = M::To(o);
            continue;
        }
        if os {
            final_matches[f] = M::X;
        } else {
            orig_matches[o] = M::X;
        }
    }

    let mut orig_rows: Vec<i64> = (0..orig.len() as i64).collect();
    let mut final_rows: Vec<i64> = (0..fin.len() as i64).collect();
    for orig_row in 0..orig.len() as i64 {
        let final_pos = match orig_matches[orig_row as usize] {
            M::To(f) => final_rows.iter().position(|&r| r == f as i64),
            _ => None,
        };
        let orig_pos = orig_rows.iter().position(|&r| r == orig_row).unwrap_or(0);
        match final_pos {
            None => {
                final_rows.insert(orig_pos.min(final_rows.len()), -1);
            }
            Some(fp) if fp < orig_pos => {
                // range( -1, $final_pos - $orig_pos )
                let pad: Vec<i64> = (1..=(orig_pos - fp) as i64).map(|k| -k).collect();
                splice_at(&mut final_rows, orig_pos, pad);
            }
            Some(fp) if fp > orig_pos => {
                let pad: Vec<i64> = (1..=(fp - orig_pos) as i64).map(|k| -k).collect();
                splice_at(&mut orig_rows, orig_pos, pad);
            }
            Some(_) => {}
        }
    }
    let diff_count = orig_rows.len() as i64 - final_rows.len() as i64;
    if diff_count < 0 {
        let mut d = diff_count;
        while d < 0 {
            orig_rows.push(d);
            d += 1;
        }
    } else if diff_count > 0 {
        let mut d = -diff_count;
        while d < 0 {
            final_rows.push(d);
            d += 1;
        }
    }

    let matched = orig_matches
        .iter()
        .map(|m| if let M::To(f) = m { Some(*f) } else { None })
        .collect();
    (matched, orig_rows, final_rows)
}

/// `array_splice( $rows, $at, 0, $values )`: an offset past the end appends.
fn splice_at(rows: &mut Vec<i64>, at: usize, values: Vec<i64>) {
    let at = at.min(rows.len());
    rows.splice(at..at, values);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pairs_closest_lines_and_pads() {
        let (m, o, f) = interleave(
            &[b"alpha beta", b"gamma"],
            &[b"gamma!", b"zzz", b"alpha beta!"],
        );
        assert_eq!(m, [Some(2), Some(0)]);
        assert_eq!(o.len(), f.len());
    }
}
