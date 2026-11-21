use speedyspec_routing::{CompiledDispatch, DispatchStrategy};

use super::Singles;

/// Asks a `RegexSet` per chunk which patterns match, then runs the first for its captures.
#[derive(Debug, Clone, Copy)]
pub struct RegexSetStrategy {
    pub chunk_size: usize,
}

impl Default for RegexSetStrategy {
    fn default() -> RegexSetStrategy {
        RegexSetStrategy { chunk_size: 10 }
    }
}

enum Chunk {
    Set {
        start: usize,
        end: usize,
        set: regex::RegexSet,
    },
    Single {
        index: usize,
    },
}

struct CompiledSets {
    singles: Singles,
    chunks: Vec<Chunk>,
}

impl RegexSetStrategy {
    fn combine(patterns: &[String], start: usize, end: usize, out: &mut Vec<Chunk>) {
        if end - start == 1 {
            out.push(Chunk::Single { index: start });
            return;
        }
        match regex::RegexSet::new(&patterns[start..end]) {
            Ok(set) => out.push(Chunk::Set { start, end, set }),
            Err(_) => {
                let middle = start + (end - start) / 2;
                RegexSetStrategy::combine(patterns, start, middle, out);
                RegexSetStrategy::combine(patterns, middle, end, out);
            }
        }
    }
}

impl DispatchStrategy for RegexSetStrategy {
    fn build(&self, patterns: &[String]) -> Result<Box<dyn CompiledDispatch>, String> {
        let singles = Singles::compile(patterns);
        let mut chunks = Vec::new();
        for (start, end, combinable) in singles.runs(self.chunk_size) {
            if combinable {
                RegexSetStrategy::combine(patterns, start, end, &mut chunks);
            } else {
                chunks.push(Chunk::Single { index: start });
            }
        }
        Ok(Box::new(CompiledSets { singles, chunks }))
    }
}

impl CompiledDispatch for CompiledSets {
    fn find(&self, subject: &str, from: usize) -> Option<(usize, Vec<Option<String>>)> {
        for chunk in &self.chunks {
            let (start, end) = match chunk {
                Chunk::Set { start, end, .. } => (*start, *end),
                Chunk::Single { index } => (*index, *index + 1),
            };
            if end <= from {
                continue;
            }
            match chunk {
                Chunk::Single { index } => {
                    if let Some(caps) = self.singles.0[*index].captures(subject) {
                        return Some((*index, caps));
                    }
                }
                Chunk::Set { set, .. } => {
                    let first = set
                        .matches(subject)
                        .iter()
                        .map(|k| start + k)
                        .find(|&i| i >= from);
                    if let Some(i) = first
                        && let Some(caps) = self.singles.0[i].captures(subject)
                    {
                        return Some((i, caps));
                    }
                }
            }
        }
        None
    }

    fn first_needs_php(&self, from: usize) -> Option<usize> {
        self.singles.first_needs_php(from)
    }
}
