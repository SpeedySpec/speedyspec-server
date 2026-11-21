//! Dispatch strategies: how an ordered list of regexes finds its first match in a few regex runs (Strategy).
//!
//! Both strategies group consecutive patterns into chunks. [`ChunkedAlternation`] joins a chunk into one alternation
//! and reads which alternative matched from a marker group, FastRoute's group-count idea adapted to Rust's global
//! group numbering. [`RegexSetStrategy`] asks a `RegexSet` which patterns match and runs the first one for its
//! captures. A pattern only `fancy-regex` compiles sits alone in its chunk, so order is kept; a pattern no engine
//! compiles is reported by `first_needs_php`.

mod chunked;
mod engine;
mod regex_set;

pub use chunked::ChunkedAlternation;
pub use engine::{EngineChoice, engine_for};
pub use regex_set::RegexSetStrategy;

use engine::Compiled;

use crate::pcre::has_top_level_alternation;

/// Patterns compiled one by one, shared by both strategies for resuming inside a chunk and for single-pattern
/// chunks.
struct Singles(Vec<Compiled>, Vec<bool>);

impl Singles {
    /// Compiles each pattern, noting which may join a chunk: a `regex`-crate pattern with no top-level alternation.
    /// An alternative after the first is unanchored, so a combined alternation could prefer a later pattern matching
    /// further left.
    fn compile(patterns: &[String]) -> Singles {
        let compiled: Vec<Compiled> = patterns.iter().map(|p| Compiled::new(p)).collect();
        let combinable = patterns
            .iter()
            .zip(&compiled)
            .map(|(p, c)| matches!(c, Compiled::Regex(_)) && !has_top_level_alternation(p))
            .collect();
        Singles(compiled, combinable)
    }

    fn find_between(
        &self,
        subject: &[u8],
        from: usize,
        end: usize,
    ) -> Option<(usize, Vec<Option<String>>)> {
        (from..end).find_map(|i| self.0[i].captures(subject).map(|caps| (i, caps)))
    }

    fn first_needs_php(&self, from: usize) -> Option<usize> {
        (from..self.0.len()).find(|&i| matches!(self.0[i], Compiled::None))
    }

    /// Runs of consecutive combinable patterns, at most `size` long; every other pattern is a run of one.
    fn runs(&self, size: usize) -> Vec<(usize, usize, bool)> {
        let size = size.max(1);
        let mut runs = Vec::new();
        let mut i = 0;
        while i < self.0.len() {
            if self.1[i] {
                let start = i;
                while i < self.0.len() && i - start < size && self.1[i] {
                    i += 1;
                }
                runs.push((start, i, true));
            } else {
                runs.push((i, i + 1, false));
                i += 1;
            }
        }
        runs
    }
}
