use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use aho_corasick::{AhoCorasick, Input, MatchKind};
use speedyspec_wp_shortcode_contracts::{
    ShortcodeError, ShortcodeMatch, ShortcodeMatches, ShortcodeScanner, TagName, TagSet,
};

/// The default scanner: `memchr` finds brackets with SIMD and `aho-corasick` finds names.
///
/// It follows PCRE's semantics for `get_shortcode_regex()` step by step, including backtracking into the name
/// alternation, so it agrees with [`crate::FancyRegexScanner`] on every input. Closer and `]` searches are
/// remembered, so many openers with no closer stay linear.
#[derive(Default)]
pub struct HandScanner {
    cache: RefCell<Option<Rc<Compiled>>>,
}

struct Compiled {
    generation: u64,
    names: Vec<TagName>,
    automaton: AhoCorasick,
    longest: usize,
}

impl HandScanner {
    /// A scanner with an empty cache.
    pub fn new() -> Self {
        Self::default()
    }

    fn compiled(&self, tags: &TagSet) -> Result<Rc<Compiled>, ShortcodeError> {
        if let Some(c) = &*self.cache.borrow()
            && c.generation == tags.generation()
            && c.names.as_slice() == tags.names()
        {
            return Ok(c.clone());
        }
        // An empty set is PCRE's empty group `()`, which matches an empty name.
        let names: Vec<TagName> = if tags.names().is_empty() {
            vec![TagName::from_global(Vec::new())]
        } else {
            tags.names().to_vec()
        };
        let automaton = AhoCorasick::builder()
            .match_kind(MatchKind::Standard)
            .build(names.iter().map(TagName::as_bytes))
            .map_err(|e| ShortcodeError::ScanFailed(e.to_string()))?;
        let longest = names.iter().map(|n| n.as_bytes().len()).max().unwrap_or(0);
        let compiled = Rc::new(Compiled { generation: tags.generation(), names, automaton, longest });
        *self.cache.borrow_mut() = Some(compiled.clone());
        Ok(compiled)
    }
}

impl ShortcodeScanner for HandScanner {
    fn scan<'h>(&self, haystack: &'h [u8], tags: &TagSet) -> ShortcodeMatches<'h> {
        match self.compiled(tags) {
            Ok(compiled) => Box::new(HandMatches {
                compiled,
                haystack,
                pos: 0,
                next_close: None,
                closers: HashMap::new(),
            }),
            Err(e) => Box::new(std::iter::once(Err(e))),
        }
    }
}

/// What a forward search found, from where: `found` is `None` when nothing exists at or after `from`.
#[derive(Clone, Copy)]
struct Memo {
    from: usize,
    found: Option<usize>,
}

impl Memo {
    /// Reuses an earlier search when it covers `start`.
    fn answers(self, start: usize) -> Option<Option<usize>> {
        match self.found {
            Some(at) if start >= self.from && start <= at => Some(Some(at)),
            None if start >= self.from => Some(None),
            _ => None,
        }
    }
}

struct HandMatches<'h> {
    compiled: Rc<Compiled>,
    haystack: &'h [u8],
    pos: usize,
    next_close: Option<Memo>,
    closers: HashMap<usize, Memo>,
}

fn is_word_or_hyphen(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'-'
}

impl HandMatches<'_> {
    /// The first `]` at or after `start`.
    fn close_bracket(&mut self, start: usize) -> Option<usize> {
        if let Some(answer) = self.next_close.and_then(|m| m.answers(start)) {
            return answer;
        }
        let found = memchr::memchr(b']', &self.haystack[start..]).map(|i| start + i);
        self.next_close = Some(Memo { from: start, found });
        found
    }

    /// The first `[/name]` at or after `start`, for the name at index `id`.
    fn closer(&mut self, id: usize, start: usize) -> Option<usize> {
        if let Some(answer) = self.closers.get(&id).and_then(|m| m.answers(start)) {
            return answer;
        }
        let name = self.compiled.names[id].as_bytes();
        let mut needle = Vec::with_capacity(name.len() + 3);
        needle.extend_from_slice(b"[/");
        needle.extend_from_slice(name);
        needle.push(b']');
        let found = memchr::memmem::find(&self.haystack[start..], &needle).map(|i| start + i);
        self.closers.insert(id, Memo { from: start, found });
        found
    }

    /// Names that are a prefix of the haystack at `at`, in alternation order.
    fn candidates(&self, at: usize) -> Vec<usize> {
        let end = (at + self.compiled.longest).min(self.haystack.len());
        let input = Input::new(self.haystack).span(at..end);
        let mut ids: Vec<usize> = self
            .compiled
            .automaton
            .find_overlapping_iter(input)
            .filter(|m| m.start() == at)
            .map(|m| m.pattern().as_usize())
            .collect();
        ids.sort_unstable();
        ids
    }

    /// Tries the whole pattern at the `[` at `open`.
    fn match_at(&mut self, open: usize) -> Option<ShortcodeMatch> {
        let hay = self.haystack;
        let doubled = hay.get(open + 1) == Some(&b'[');
        for g1 in if doubled { &[1usize, 0][..] } else { &[0usize][..] } {
            let name_start = open + 1 + g1;
            for id in self.candidates(name_start) {
                let name_end = name_start + self.compiled.names[id].as_bytes().len();
                if hay.get(name_end).is_some_and(|&b| is_word_or_hyphen(b)) {
                    continue;
                }
                let Some(close) = self.close_bracket(name_end) else {
                    continue;
                };
                let self_closing = close > name_end && hay[close - 1] == b'/';
                let g3 = name_end..if self_closing { close - 1 } else { close };
                let g4 = self_closing.then(|| close - 1..close);
                let mut end = close + 1;
                let mut g5 = None;
                if !self_closing && let Some(at) = self.closer(id, end) {
                    g5 = Some(end..at);
                    end = at + 3 + self.compiled.names[id].as_bytes().len();
                }
                let g6_end = if hay.get(end) == Some(&b']') { end + 1 } else { end };
                return Some(ShortcodeMatch {
                    start: open,
                    end: g6_end,
                    groups: [
                        Some(open..g6_end),
                        Some(open + 1..name_start),
                        Some(name_start..name_end),
                        Some(g3),
                        g4,
                        g5,
                        Some(end..g6_end),
                    ],
                });
            }
        }
        None
    }
}

impl Iterator for HandMatches<'_> {
    type Item = Result<ShortcodeMatch, ShortcodeError>;

    fn next(&mut self) -> Option<Self::Item> {
        while self.pos < self.haystack.len() {
            let open = self.pos + memchr::memchr(b'[', &self.haystack[self.pos..])?;
            match self.match_at(open) {
                Some(m) => {
                    self.pos = m.end;
                    return Some(Ok(m));
                }
                None => self.pos = open + 1,
            }
        }
        None
    }
}
