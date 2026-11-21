/// A key of the array `shortcode_parse_atts()` returns.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum AttKey {
    Int(i64),
    Str(Vec<u8>),
}

impl AttKey {
    /// Converts a string key as PHP does: a canonical decimal integer in range becomes an int key.
    pub fn from_php(key: Vec<u8>) -> Self {
        match canonical_int(&key) {
            Some(i) => Self::Int(i),
            None => Self::Str(key),
        }
    }
}

fn canonical_int(s: &[u8]) -> Option<i64> {
    let digits = s.strip_prefix(b"-").unwrap_or(s);
    if digits.is_empty() || !digits.iter().all(u8::is_ascii_digit) {
        return None;
    }
    if digits[0] == b'0' && (digits.len() > 1 || s.len() != digits.len()) {
        return None;
    }
    std::str::from_utf8(s).ok()?.parse().ok()
}

/// The attributes of one shortcode, with PHP array semantics.
///
/// Keys keep their first position when overwritten, and [`ParsedAtts::push`] appends at PHP's next free index.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ParsedAtts {
    entries: Vec<(AttKey, Vec<u8>)>,
    next_free: Option<i64>,
}

impl ParsedAtts {
    /// An empty array.
    pub fn new() -> Self {
        Self::default()
    }

    /// `$atts[ $key ] = $value`.
    pub fn insert(&mut self, key: AttKey, value: Vec<u8>) {
        if let AttKey::Int(i) = key
            && self.next_free.is_none_or(|n| i >= n)
        {
            self.next_free = Some(i.saturating_add(1));
        }
        match self.entries.iter_mut().find(|(k, _)| *k == key) {
            Some(slot) => slot.1 = value,
            None => self.entries.push((key, value)),
        }
    }

    /// `$atts[] = $value`.
    pub fn push(&mut self, value: Vec<u8>) {
        let key = self.next_free.unwrap_or(0);
        self.insert(AttKey::Int(key), value);
    }

    /// The entries in order.
    pub fn iter(&self) -> impl Iterator<Item = (&AttKey, &[u8])> {
        self.entries.iter().map(|(k, v)| (k, v.as_slice()))
    }

    /// Mutable values in order, for the pass that blanks unclosed HTML.
    pub fn values_mut(&mut self) -> impl Iterator<Item = &mut Vec<u8>> {
        self.entries.iter_mut().map(|(_, v)| v)
    }

    /// True when there are no entries.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The array as a [`crate::PhpValue`].
    pub fn to_value(&self) -> crate::PhpValue {
        crate::PhpValue::Array(
            self.entries
                .iter()
                .map(|(k, v)| {
                    let key = match k {
                        AttKey::Int(i) => crate::PhpKey::Int(*i),
                        AttKey::Str(s) => crate::PhpKey::Str(String::from_utf8_lossy(s).into_owned()),
                    };
                    (key, crate::php_bytes(v.clone()))
                })
                .collect(),
        )
    }
}

/// What an [`crate::AttsParser`] found.
///
/// `null_subject` is true when the text was not valid UTF-8: WordPress's `/u` `preg_replace` returned `null`, and
/// passing that on raises an E_DEPRECATED the caller must report.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AttsParse {
    pub atts: ParsedAtts,
    pub null_subject: bool,
}
