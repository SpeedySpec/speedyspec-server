//! `Text_Diff_Renderer::render()`'s grouping of operations into blocks with leading and trailing context.
//!
//! WordPress's renderers ask for 10,000 lines of context each way, so in practice a diff is one block; past that,
//! a long unchanged run is cut as Horde cuts it.

use std::ops::Range;

use speedyspec_wp_text_diff_contracts::DiffOp;

/// One entry of a block.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BlockEdit {
    /// Unchanged tokens, from the original side (`$edit->orig`).
    Context(Range<usize>),
    Added(Range<usize>),
    Deleted(Range<usize>),
    Changed(Range<usize>, Range<usize>),
}

/// The context both WordPress renderers ask for.
pub const CONTEXT: usize = 10_000;

/// Groups `ops` into blocks as `render()` does, with `nlead` leading and `ntrail` trailing context tokens.
pub fn blocks(ops: &[DiffOp], nlead: usize, ntrail: usize) -> Vec<Vec<BlockEdit>> {
    let mut out = Vec::new();
    let mut block: Option<Vec<BlockEdit>> = None;
    let mut context: Range<usize> = 0..0;
    let last = ops.len().saturating_sub(1);
    for (i, op) in ops.iter().enumerate() {
        match op {
            DiffOp::Copy { orig, .. } => {
                if let Some(b) = block.as_mut() {
                    let keep = if i == last { ntrail } else { nlead + ntrail };
                    if orig.len() <= keep {
                        b.push(BlockEdit::Context(orig.clone()));
                    } else {
                        if ntrail > 0 {
                            b.push(BlockEdit::Context(orig.start..orig.start + ntrail));
                        }
                        out.push(block.take().unwrap_or_default());
                    }
                }
                context = orig.clone();
            }
            other => {
                let b = block.get_or_insert_with(|| {
                    let start = context.end - context.len().min(nlead);
                    let lead = start..context.end;
                    if lead.is_empty() {
                        Vec::new()
                    } else {
                        vec![BlockEdit::Context(lead)]
                    }
                });
                b.push(match other {
                    DiffOp::Add { fin } => BlockEdit::Added(fin.clone()),
                    DiffOp::Delete { orig } => BlockEdit::Deleted(orig.clone()),
                    DiffOp::Change { orig, fin } => BlockEdit::Changed(orig.clone(), fin.clone()),
                    DiffOp::Copy { .. } => unreachable!(),
                });
            }
        }
    }
    if let Some(b) = block {
        out.push(b);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copies_alone_render_nothing() {
        assert!(
            blocks(
                &[DiffOp::Copy {
                    orig: 0..3,
                    fin: 0..3
                }],
                CONTEXT,
                CONTEXT
            )
            .is_empty()
        );
    }

    #[test]
    fn leading_and_trailing_context_are_cut() {
        let ops = [
            DiffOp::Copy {
                orig: 0..5,
                fin: 0..5,
            },
            DiffOp::Add { fin: 5..6 },
            DiffOp::Copy {
                orig: 5..10,
                fin: 6..11,
            },
        ];
        assert_eq!(
            blocks(&ops, 2, 3),
            [vec![
                BlockEdit::Context(3..5),
                BlockEdit::Added(5..6),
                BlockEdit::Context(5..8)
            ]]
        );
    }
}
