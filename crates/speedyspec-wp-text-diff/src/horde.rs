//! Horde's `Text_Diff_Engine_native`, ported line for line, over token ids.
//!
//! The engine is Geoffrey T. Dairiki's chunked LCS with GNU diff's boundary shifting. It is not minimal: WordPress's
//! output depends on its exact choices, so every quirk of the PHP is kept (see the design's "Horde quirks"):
//!
//! - the prefix and suffix skips compare with `!==`, the hashes by exact token, while `_compareseq()` and
//!   `_shiftBoundaries()` compare with PHP's loose `==` ([`TokenTable::loose_eq`]);
//! - `while ($y = current($matches))` stops at a match at index 0;
//! - chunk bounds truncate like `(int)`; the sides flip when the first is longer.
//!
//! `_compareseq()`'s recursion is an explicit stack, so a deep input cannot overflow Rust's stack. Its subproblems
//! touch disjoint ranges and `_diag()` resets what it reads, so the order they run in is kept but does not matter.

use speedyspec_wp_text_diff_contracts::{DiffOp, SequenceDiffer};

use crate::tokens::TokenTable;

/// WordPress's diff engine (`new Text_Diff( 'native', ... )`).
#[derive(Clone, Copy, Debug, Default)]
pub struct HordeSequenceDiffer;

impl SequenceDiffer for HordeSequenceDiffer {
    fn diff(&self, orig: &[&[u8]], fin: &[&[u8]]) -> Vec<DiffOp> {
        let table = TokenTable::new(orig, fin);
        Engine::new(&table).diff()
    }
}

struct Engine<'t> {
    t: &'t TokenTable,
    xchanged: Vec<bool>,
    ychanged: Vec<bool>,
    xv: Vec<u32>,
    yv: Vec<u32>,
    xind: Vec<usize>,
    yind: Vec<usize>,
    seq: Vec<i64>,
    /// `$this->in_seq[$y]`, as a stamp: set when equal to `generation`.
    in_seq: Vec<u32>,
    generation: u32,
    lcs: usize,
}

impl<'t> Engine<'t> {
    fn new(t: &'t TokenTable) -> Engine<'t> {
        Engine {
            t,
            xchanged: Vec::new(),
            ychanged: Vec::new(),
            xv: Vec::new(),
            yv: Vec::new(),
            xind: Vec::new(),
            yind: Vec::new(),
            seq: Vec::new(),
            in_seq: Vec::new(),
            generation: 0,
            lcs: 0,
        }
    }

    fn diff(mut self) -> Vec<DiffOp> {
        let from = &self.t.from;
        let to = &self.t.to;
        let n_from = from.len();
        let n_to = to.len();
        self.xchanged = vec![false; n_from];
        self.ychanged = vec![false; n_to];

        let mut skip = 0;
        while skip < n_from && skip < n_to {
            if from[skip] != to[skip] {
                break;
            }
            skip += 1;
        }

        // for ($endskip = 0; --$xi > $skip && --$yi > $skip; $endskip++)
        let mut xi = n_from as i64;
        let mut yi = n_to as i64;
        let mut endskip = 0usize;
        loop {
            xi -= 1;
            if xi <= skip as i64 {
                break;
            }
            yi -= 1;
            if yi <= skip as i64 {
                break;
            }
            if from[xi as usize] != to[yi as usize] {
                break;
            }
            endskip += 1;
        }

        let distinct = self.t.distinct();
        let mut xhash = vec![false; distinct];
        for &line in &from[skip..n_from - endskip] {
            xhash[line as usize] = true;
        }
        let mut yhash = vec![false; distinct];
        for (yi, &line) in to.iter().enumerate().take(n_to - endskip).skip(skip) {
            self.ychanged[yi] = !xhash[line as usize];
            if self.ychanged[yi] {
                continue;
            }
            yhash[line as usize] = true;
            self.yv.push(line);
            self.yind.push(yi);
        }
        for (xi, &line) in from.iter().enumerate().take(n_from - endskip).skip(skip) {
            self.xchanged[xi] = !yhash[line as usize];
            if self.xchanged[xi] {
                continue;
            }
            self.xv.push(line);
            self.xind.push(xi);
        }

        self.compare_seq(0, self.xv.len(), 0, self.yv.len());

        let mut xchanged = std::mem::take(&mut self.xchanged);
        let mut ychanged = std::mem::take(&mut self.ychanged);
        shift_boundaries(self.t, from, &mut xchanged, &ychanged);
        shift_boundaries(self.t, to, &mut ychanged, &xchanged);

        group(&xchanged, &ychanged)
    }

    fn compare_seq(&mut self, xoff: usize, xlim: usize, yoff: usize, ylim: usize) {
        let mut stack = vec![(xoff, xlim, yoff, ylim)];
        while let Some((mut xoff, mut xlim, mut yoff, mut ylim)) = stack.pop() {
            while xoff < xlim && yoff < ylim && self.t.loose_eq(self.xv[xoff], self.yv[yoff]) {
                xoff += 1;
                yoff += 1;
            }
            while xlim > xoff
                && ylim > yoff
                && self.t.loose_eq(self.xv[xlim - 1], self.yv[ylim - 1])
            {
                xlim -= 1;
                ylim -= 1;
            }

            let (lcs, seps) = if xoff == xlim || yoff == ylim {
                (0, Vec::new())
            } else {
                let nchunks = (xlim - xoff).min(ylim - yoff).min(7) + 1;
                self.diag(xoff, xlim, yoff, ylim, nchunks)
            };

            if lcs == 0 {
                while yoff < ylim {
                    self.ychanged[self.yind[yoff]] = true;
                    yoff += 1;
                }
                while xoff < xlim {
                    self.xchanged[self.xind[xoff]] = true;
                    xoff += 1;
                }
            } else {
                // Subproblems in order: push them reversed.
                for pair in seps.windows(2).rev() {
                    let (p1, p2) = (pair[0], pair[1]);
                    stack.push((p1.0, p2.0, p1.1, p2.1));
                }
            }
        }
    }

    fn diag(
        &mut self,
        xoff: usize,
        xlim: usize,
        yoff: usize,
        ylim: usize,
        nchunks: usize,
    ) -> (usize, Vec<(usize, usize)>) {
        let flip = xlim - xoff > ylim - yoff;
        let (xoff, xlim, yoff, ylim) = if flip {
            (yoff, ylim, xoff, xlim)
        } else {
            (xoff, xlim, yoff, ylim)
        };

        // $ymatches[$line][] = $i, for $i from $ylim - 1 down: each list is in descending order.
        let other = if flip { &self.xv } else { &self.yv };
        let mut ymatches: hashbrown::HashMap<u32, Vec<usize>, foldhash::fast::FixedState> =
            hashbrown::HashMap::with_hasher(foldhash::fast::FixedState::default());
        for i in (yoff..ylim).rev() {
            ymatches.entry(other[i]).or_default().push(i);
        }

        self.lcs = 0;
        if self.seq.is_empty() {
            self.seq.push(0);
        }
        self.seq[0] = yoff as i64 - 1;
        self.generation = self.generation.wrapping_add(1);
        if self.generation == 0 {
            self.in_seq.iter_mut().for_each(|s| *s = 0);
            self.generation = 1;
        }
        let needed = ylim.max(self.in_seq.len());
        self.in_seq.resize(needed, 0);
        let mut ymids: Vec<Vec<i64>> = vec![Vec::new()];

        let numer = xlim - xoff + nchunks - 1;
        let mut x = xoff;
        let mut k = 0usize;
        for chunk in 0..nchunks {
            if chunk > 0 {
                for i in 0..=self.lcs {
                    set(&mut ymids, i, chunk - 1, self.seq[i]);
                }
            }

            let x1 = xoff + (numer + (xlim - xoff) * chunk) / nchunks;
            while x < x1 {
                let line = if flip { self.yv[x] } else { self.xv[x] };
                x += 1;
                let Some(matches) = ymatches.get(&line) else {
                    continue;
                };
                let mut it = 0;
                // while ($y = current($matches)): a match at 0 ends the loop.
                while it < matches.len() && matches[it] != 0 {
                    let y = matches[it];
                    if !self.in_seq(y) {
                        k = self.lcs_pos(y as i64);
                        let prev = ymids[k - 1].clone();
                        put(&mut ymids, k, prev);
                        break;
                    }
                    it += 1;
                }
                while it < matches.len() && matches[it] != 0 {
                    let y = matches[it];
                    if y as i64 > self.seq[k - 1] {
                        // assert($y <= $this->seq[$k]);
                        let old = self.seq[k];
                        self.clear_in_seq(old);
                        self.seq[k] = y as i64;
                        self.mark_in_seq(y);
                    } else if !self.in_seq(y) {
                        k = self.lcs_pos(y as i64);
                        let prev = ymids[k - 1].clone();
                        put(&mut ymids, k, prev);
                    }
                    it += 1;
                }
            }
        }

        let mut seps = Vec::with_capacity(nchunks + 1);
        seps.push(if flip { (yoff, xoff) } else { (xoff, yoff) });
        let ymid = ymids[self.lcs].clone();
        for n in 0..nchunks - 1 {
            let x1 = xoff + (numer + (xlim - xoff) * n) / nchunks;
            let y1 = (ymid.get(n).copied().unwrap_or(0) + 1) as usize;
            seps.push(if flip { (y1, x1) } else { (x1, y1) });
        }
        seps.push(if flip { (ylim, xlim) } else { (xlim, ylim) });
        (self.lcs, seps)
    }

    #[inline]
    fn in_seq(&self, y: usize) -> bool {
        self.in_seq.get(y).is_some_and(|&g| g == self.generation)
    }

    #[inline]
    fn mark_in_seq(&mut self, y: usize) {
        if y >= self.in_seq.len() {
            self.in_seq.resize(y + 1, 0);
        }
        self.in_seq[y] = self.generation;
    }

    #[inline]
    fn clear_in_seq(&mut self, y: i64) {
        if y >= 0
            && let Some(slot) = self.in_seq.get_mut(y as usize)
        {
            *slot = 0;
        }
    }

    fn lcs_pos(&mut self, ypos: i64) -> usize {
        let end = self.lcs;
        if end == 0 || ypos > self.seq[end] {
            self.lcs += 1;
            let lcs = self.lcs;
            if self.seq.len() <= lcs {
                self.seq.resize(lcs + 1, 0);
            }
            self.seq[lcs] = ypos;
            self.mark_in_seq(ypos as usize);
            return lcs;
        }
        let mut beg = 1;
        let mut end = end;
        while beg < end {
            let mid = (beg + end) / 2;
            if ypos > self.seq[mid] {
                beg = mid + 1;
            } else {
                end = mid;
            }
        }
        let old = self.seq[end];
        self.clear_in_seq(old);
        self.seq[end] = ypos;
        self.mark_in_seq(ypos as usize);
        end
    }
}

/// `$ymids[$i][$chunk] = $value`, growing the arrays as PHP would.
fn set(ymids: &mut Vec<Vec<i64>>, i: usize, chunk: usize, value: i64) {
    if ymids.len() <= i {
        ymids.resize(i + 1, Vec::new());
    }
    let row = &mut ymids[i];
    if row.len() <= chunk {
        row.resize(chunk + 1, 0);
    }
    row[chunk] = value;
}

/// `$ymids[$k] = $ymids[$k - 1]`.
fn put(ymids: &mut Vec<Vec<i64>>, k: usize, row: Vec<i64>) {
    if ymids.len() <= k {
        ymids.resize(k + 1, Vec::new());
    }
    ymids[k] = row;
}

/// `_shiftBoundaries()`: slides each run of changes back, then forward, to line it up with the other side's
/// changes, comparing tokens with PHP's loose `==`.
fn shift_boundaries(t: &TokenTable, lines: &[u32], changed: &mut [bool], other: &[bool]) {
    let len = lines.len();
    let other_len = other.len();
    let oc = |j: isize| j >= 0 && (j as usize) < other_len && other[j as usize];
    let eq = |a: usize, b: usize| t.loose_eq(lines[a], lines[b]);
    let mut i = 0usize;
    let mut j = 0isize;
    loop {
        while oc(j) {
            j += 1;
        }
        while i < len && !changed[i] {
            i += 1;
            j += 1;
            while oc(j) {
                j += 1;
            }
        }
        if i == len {
            break;
        }
        let mut start = i;
        i += 1;
        while i < len && changed[i] {
            i += 1;
        }
        let mut corresponding;
        loop {
            let runlength = i - start;
            while start > 0 && eq(start - 1, i - 1) {
                start -= 1;
                changed[start] = true;
                i -= 1;
                changed[i] = false;
                while start > 0 && changed[start - 1] {
                    start -= 1;
                }
                // while ($other_changed[--$j]) continue;
                loop {
                    j -= 1;
                    if !oc(j) {
                        break;
                    }
                }
            }
            corresponding = if j < other_len as isize { i } else { len };
            while i < len && eq(start, i) {
                changed[start] = false;
                start += 1;
                changed[i] = true;
                i += 1;
                while i < len && changed[i] {
                    i += 1;
                }
                j += 1;
                if oc(j) {
                    corresponding = i;
                    while oc(j) {
                        j += 1;
                    }
                }
            }
            if runlength == i - start {
                break;
            }
        }
        while corresponding < i {
            start -= 1;
            changed[start] = true;
            i -= 1;
            changed[i] = false;
            loop {
                j -= 1;
                if !oc(j) {
                    break;
                }
            }
        }
    }
}

/// Horde's final loop: changed flags into copy, delete, add and change operations.
fn group(xchanged: &[bool], ychanged: &[bool]) -> Vec<DiffOp> {
    let (n_from, n_to) = (xchanged.len(), ychanged.len());
    let (mut xi, mut yi) = (0, 0);
    let mut edits = Vec::new();
    while xi < n_from || yi < n_to {
        let (x0, y0) = (xi, yi);
        while xi < n_from && yi < n_to && !xchanged[xi] && !ychanged[yi] {
            xi += 1;
            yi += 1;
        }
        if xi > x0 {
            edits.push(DiffOp::Copy {
                orig: x0..xi,
                fin: y0..yi,
            });
        }
        let x1 = xi;
        while xi < n_from && xchanged[xi] {
            xi += 1;
        }
        let y1 = yi;
        while yi < n_to && ychanged[yi] {
            yi += 1;
        }
        match (xi > x1, yi > y1) {
            (true, true) => edits.push(DiffOp::Change {
                orig: x1..xi,
                fin: y1..yi,
            }),
            (true, false) => edits.push(DiffOp::Delete { orig: x1..xi }),
            (false, true) => edits.push(DiffOp::Add { fin: y1..yi }),
            (false, false) => {}
        }
    }
    edits
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ops(a: &[&str], b: &[&str]) -> Vec<DiffOp> {
        let a: Vec<&[u8]> = a.iter().map(|s| s.as_bytes()).collect();
        let b: Vec<&[u8]> = b.iter().map(|s| s.as_bytes()).collect();
        HordeSequenceDiffer.diff(&a, &b)
    }

    #[test]
    fn identical_is_one_copy() {
        assert_eq!(
            ops(&["a", "b"], &["a", "b"]),
            [DiffOp::Copy {
                orig: 0..2,
                fin: 0..2
            }]
        );
    }

    #[test]
    fn one_line_changed() {
        assert_eq!(
            ops(&["a", "b", "c"], &["a", "x", "c"]),
            [
                DiffOp::Copy {
                    orig: 0..1,
                    fin: 0..1
                },
                DiffOp::Change {
                    orig: 1..2,
                    fin: 1..2
                },
                DiffOp::Copy {
                    orig: 2..3,
                    fin: 2..3
                },
            ]
        );
    }

    #[test]
    fn empty_sides() {
        assert_eq!(ops(&[], &["a"]), [DiffOp::Add { fin: 0..1 }]);
        assert_eq!(ops(&["a"], &[]), [DiffOp::Delete { orig: 0..1 }]);
        assert!(ops(&[], &[]).is_empty());
    }
}
