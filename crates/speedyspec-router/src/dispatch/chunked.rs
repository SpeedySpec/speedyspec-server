use speedyspec_routing::{CompiledDispatch, DispatchStrategy};

use super::Singles;

/// Joins each chunk of patterns into one alternation; a marker group after each alternative names the winner.
#[derive(Debug, Clone, Copy)]
pub struct ChunkedAlternation {
    pub chunk_size: usize,
    /// The engine's compiled-size limit for one chunk; `None` keeps the `regex` crate's default. A chunk over it is
    /// halved until each part fits.
    pub size_limit: Option<usize>,
}

impl Default for ChunkedAlternation {
    fn default() -> ChunkedAlternation {
        ChunkedAlternation {
            chunk_size: 10,
            size_limit: None,
        }
    }
}

/// Where one alternative's groups sit in its chunk's combined regex.
struct Alternative {
    first_group: usize,
    groups: usize,
    marker: usize,
}

enum Chunk {
    Combined {
        start: usize,
        end: usize,
        regex: regex::bytes::Regex,
        alternatives: Vec<Alternative>,
    },
    Single {
        index: usize,
    },
}

impl Chunk {
    fn range(&self) -> (usize, usize) {
        match self {
            Chunk::Combined { start, end, .. } => (*start, *end),
            Chunk::Single { index } => (*index, *index + 1),
        }
    }
}

struct CompiledChunks {
    singles: Singles,
    chunks: Vec<Chunk>,
}

impl ChunkedAlternation {
    /// Combines `start..end`, halving the run when the engine refuses the combination (its size limit).
    fn combine(
        &self,
        singles: &Singles,
        patterns: &[String],
        start: usize,
        end: usize,
        out: &mut Vec<Chunk>,
    ) {
        if end - start == 1 {
            out.push(Chunk::Single { index: start });
            return;
        }
        let mut source = String::new();
        let mut alternatives = Vec::with_capacity(end - start);
        let mut group = 1;
        for (offset, pattern) in patterns[start..end].iter().enumerate() {
            if offset > 0 {
                source.push('|');
            }
            source.push_str(&format!("(?:(?:{pattern})())"));
            let groups = singles.0[start + offset].group_count();
            alternatives.push(Alternative {
                first_group: group,
                groups,
                marker: group + groups,
            });
            group += groups + 1;
        }
        let mut builder = regex::bytes::RegexBuilder::new(&source);
        builder.unicode(false);
        if let Some(limit) = self.size_limit {
            builder.size_limit(limit);
        }
        match builder.build() {
            Ok(regex) => out.push(Chunk::Combined {
                start,
                end,
                regex,
                alternatives,
            }),
            Err(_) => {
                let middle = start + (end - start) / 2;
                self.combine(singles, patterns, start, middle, out);
                self.combine(singles, patterns, middle, end, out);
            }
        }
    }
}

impl DispatchStrategy for ChunkedAlternation {
    fn build(&self, patterns: &[String]) -> Result<Box<dyn CompiledDispatch>, String> {
        let singles = Singles::compile(patterns);
        let mut chunks = Vec::new();
        for (start, end, combinable) in singles.runs(self.chunk_size) {
            if combinable {
                self.combine(&singles, patterns, start, end, &mut chunks);
            } else {
                chunks.push(Chunk::Single { index: start });
            }
        }
        Ok(Box::new(CompiledChunks { singles, chunks }))
    }
}

impl CompiledDispatch for CompiledChunks {
    fn find(&self, subject: &[u8], from: usize) -> Option<(usize, Vec<Option<String>>)> {
        for chunk in &self.chunks {
            let (start, end) = chunk.range();
            if end <= from {
                continue;
            }
            if start < from {
                if let Some(found) = self.singles.find_between(subject, from, end) {
                    return Some(found);
                }
                continue;
            }
            match chunk {
                Chunk::Single { index } => {
                    if let Some(caps) = self.singles.0[*index].captures(subject) {
                        return Some((*index, caps));
                    }
                }
                Chunk::Combined {
                    start,
                    regex,
                    alternatives,
                    ..
                } => {
                    let Some(caps) = regex.captures(subject) else {
                        continue;
                    };
                    let Some(k) = alternatives
                        .iter()
                        .position(|a| caps.get(a.marker).is_some())
                    else {
                        continue;
                    };
                    let alternative = &alternatives[k];
                    let mut out = Vec::with_capacity(alternative.groups + 1);
                    let text = |m: regex::bytes::Match<'_>| {
                        String::from_utf8_lossy(m.as_bytes()).into_owned()
                    };
                    out.push(caps.get(0).map(text));
                    for g in alternative.first_group..alternative.first_group + alternative.groups {
                        out.push(caps.get(g).map(text));
                    }
                    return Some((start + k, out));
                }
            }
        }
        None
    }

    fn first_needs_php(&self, from: usize) -> Option<usize> {
        self.singles.first_needs_php(from)
    }
}
