/// A shortcode tag name, as a key of `$shortcode_tags`.
///
/// [`TagName::new`] validates as `add_shortcode()` does. [`TagName::from_global`] accepts anything, because code
/// may write any key straight into the global, and `has_shortcode()` still matches it.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TagName(Vec<u8>);

/// Why `add_shortcode()` refuses a name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InvalidTagName {
    /// The name is empty after PHP's `trim()`.
    Empty,
    /// The name contains one of `< > & / [ ] =` or a byte from 0x00 to 0x20.
    Reserved,
}

impl TagName {
    /// Validates a name as `add_shortcode()` does.
    ///
    /// # Errors
    ///
    /// [`InvalidTagName`] when WordPress would call `_doing_it_wrong()` and not register it.
    pub fn new(name: &[u8]) -> Result<Self, InvalidTagName> {
        if name.iter().all(|b| matches!(b, b' ' | b'\t' | b'\n' | b'\r' | 0 | 0x0B)) {
            return Err(InvalidTagName::Empty);
        }
        if name.iter().any(|&b| is_reserved(b)) {
            return Err(InvalidTagName::Reserved);
        }
        Ok(Self(name.to_vec()))
    }

    /// Takes a key of `$shortcode_tags` as it is.
    pub fn from_global(name: Vec<u8>) -> Self {
        Self(name)
    }

    /// The name's bytes.
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

/// A byte `add_shortcode()` refuses, and that ends a candidate token in `do_shortcode()`.
pub(crate) fn is_reserved(b: u8) -> bool {
    matches!(b, b'<' | b'>' | b'&' | b'/' | b'[' | b']' | b'=') || b <= 0x20
}

/// The names one scan matches, in the order PCRE's alternation tries them.
///
/// `generation` is the registry's generation the set was taken from, so scanners can cache automata.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TagSet {
    names: Vec<TagName>,
    generation: u64,
}

impl TagSet {
    /// Builds a set from names in alternation order.
    pub fn new(names: Vec<TagName>, generation: u64) -> Self {
        Self { names, generation }
    }

    /// The names, in alternation order.
    pub fn names(&self) -> &[TagName] {
        &self.names
    }

    /// The registry generation this set came from.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// True when "the first valid name wins" may not hold, so a scanner must try every candidate.
    ///
    /// That is when the set is empty (PCRE then matches an empty name) or any name contains `]`, `/` or `[`.
    pub fn has_unsafe_names(&self) -> bool {
        self.names.is_empty()
            || self
                .names
                .iter()
                .any(|n| n.0.iter().any(|b| matches!(b, b']' | b'/' | b'[')))
    }
}
