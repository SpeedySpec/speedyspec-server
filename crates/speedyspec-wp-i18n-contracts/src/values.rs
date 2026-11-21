//! Plain values: a translation file's identity, its raw contents as a reader produced them, and read errors.

use std::fmt;
use std::ops::Range;
use std::path::PathBuf;

/// What identifies one version of a translation file on disk: a file rewritten in place changes its size or
/// modification time, and a file replaced by rename changes its inode.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct FileIdentity {
    /// The canonical path (`realpath`).
    pub path: PathBuf,
    pub dev: u64,
    pub ino: u64,
    pub size: u64,
    pub mtime_ns: i128,
}

/// One entry of a translation file, as byte ranges into [`RawTranslations::bytes`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawEntry {
    /// The lookup key: `context\x04singular`. The MO reader drops the plural part (after `\0`), as
    /// `WP_Translation_File_MO` does; `.l10n.php` keys are used as written.
    pub key: Range<u32>,
    /// Every translation, joined by `\0`.
    pub value: Range<u32>,
}

/// A translation file's contents as a reader produced them, before any index is built.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RawTranslations {
    /// The bytes every entry's ranges point into.
    pub bytes: Vec<u8>,
    /// Header names lower-cased, in file order.
    pub headers: Vec<(String, String)>,
    /// Entries in file order; duplicates are kept, as the file has them.
    pub entries: Vec<RawEntry>,
}

impl RawTranslations {
    pub fn key(&self, entry: &RawEntry) -> &[u8] {
        &self.bytes[entry.key.start as usize..entry.key.end as usize]
    }

    pub fn value(&self, entry: &RawEntry) -> &[u8] {
        &self.bytes[entry.value.start as usize..entry.value.end as usize]
    }
}

/// Why a translation file could not be read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReadError {
    /// The file could not be read at all.
    Unreadable,
    /// The file is not a valid translation file; the text is WordPress's own error (`Invalid data`, `Magic marker
    /// does not exist`, `Unsupported revision`).
    Invalid(&'static str),
    /// A `.l10n.php` file outside the grammar the generators write; PHP can still `include` it.
    Unsupported,
}

impl fmt::Display for ReadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ReadError::Unreadable => f.write_str("Unreadable file"),
            ReadError::Invalid(message) => f.write_str(message),
            ReadError::Unsupported => f.write_str("Unsupported PHP translation file"),
        }
    }
}

impl std::error::Error for ReadError {}
