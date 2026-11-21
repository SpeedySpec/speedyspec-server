//! A loaded translation file (`WP_Translation_File`'s data): its bytes, headers, a lookup index and the plural rule.

use std::cell::OnceCell;
use std::hash::BuildHasher;
use std::ops::Range;

use hashbrown::HashTable;
use speedyspec_wp_i18n_contracts::{FileIdentity, RawTranslations, ReadError};

use crate::plural::{PluralError, PluralRule};

/// A translation file, read once and shared, immutable, by every request and domain that loads it (Flyweight).
pub struct TranslationFile {
    identity: FileIdentity,
    bytes: Vec<u8>,
    headers: Vec<(String, String)>,
    /// Unique keys in PHP array order (first insertion), each with its last value.
    entries: Vec<(Range<u32>, Range<u32>)>,
    index: HashTable<u32>,
    hasher: foldhash::fast::FixedState,
    plural: OnceCell<Option<PluralRule>>,
    error: Option<ReadError>,
}

impl TranslationFile {
    /// Indexes `raw`. A later entry with the same key replaces the earlier one's value but keeps its position, as
    /// PHP array assignment does.
    pub fn new(identity: FileIdentity, raw: RawTranslations) -> TranslationFile {
        let hasher = foldhash::fast::FixedState::with_seed(0x6c31_306e);
        let RawTranslations {
            bytes,
            headers,
            entries: raw_entries,
        } = raw;
        let mut entries: Vec<(Range<u32>, Range<u32>)> = Vec::with_capacity(raw_entries.len());
        let mut index: HashTable<u32> = HashTable::with_capacity(raw_entries.len());
        let slice = |r: &Range<u32>| &bytes[r.start as usize..r.end as usize];
        for e in raw_entries {
            let key = slice(&e.key);
            let h = hasher.hash_one(key);
            match index.find(h, |&i| slice(&entries[i as usize].0) == key) {
                Some(&i) => entries[i as usize].1 = e.value,
                None => {
                    let i = entries.len() as u32;
                    entries.push((e.key, e.value));
                    index.insert_unique(h, i, |&j| hasher.hash_one(slice(&entries[j as usize].0)));
                }
            }
        }
        TranslationFile {
            identity,
            bytes,
            headers,
            entries,
            index,
            hasher,
            plural: OnceCell::new(),
            error: None,
        }
    }

    /// A file that could not be read; it translates nothing and reports `error`.
    pub fn failed(identity: FileIdentity, error: ReadError) -> TranslationFile {
        let mut file = TranslationFile::new(identity, RawTranslations::default());
        file.error = Some(error);
        file
    }

    fn slice(&self, r: &Range<u32>) -> &[u8] {
        &self.bytes[r.start as usize..r.end as usize]
    }

    /// The whole translation for a lookup key (`context\x04singular`), every form joined by `\0`.
    #[inline]
    pub fn translate(&self, key: &[u8]) -> Option<&[u8]> {
        let h = self.hasher.hash_one(key);
        self.index
            .find(h, |&i| self.slice(&self.entries[i as usize].0) == key)
            .map(|&i| self.slice(&self.entries[i as usize].1))
    }

    /// The first form only, up to the first `\0`.
    #[inline]
    pub fn first_form(&self, key: &[u8]) -> Option<&[u8]> {
        self.translate(key)
            .map(|v| v.split(|b| *b == 0).next().unwrap_or(v))
    }

    /// Every form, in order.
    pub fn forms(&self, key: &[u8]) -> Option<impl Iterator<Item = &[u8]>> {
        self.translate(key).map(|v| v.split(|b| *b == 0))
    }

    /// Header names lower-cased, in file order.
    pub fn headers(&self) -> &[(String, String)] {
        &self.headers
    }

    /// Every entry as `(key, translation)`, in PHP array order.
    pub fn entries(&self) -> impl Iterator<Item = (&[u8], &[u8])> {
        self.entries
            .iter()
            .map(|(k, v)| (self.slice(k), self.slice(v)))
    }

    pub fn identity(&self) -> &FileIdentity {
        &self.identity
    }

    pub fn error(&self) -> Option<&ReadError> {
        self.error.as_ref()
    }

    /// The plural form for `n`, as `WP_Translation_File::get_plural_form()` computes it: the `plural-forms`
    /// header's rule, compiled on first use; without the header, English.
    pub fn plural_form(&self, n: i64) -> Result<i64, PluralError> {
        let rule = self.plural.get_or_init(|| {
            self.headers
                .iter()
                .rev()
                .find(|(k, _)| k == "plural-forms")
                .map(|(_, v)| PluralRule::from_header(v))
        });
        match rule {
            Some(rule) => rule.eval(n),
            None => Ok(if n == 1 { 0 } else { 1 }),
        }
    }
}
