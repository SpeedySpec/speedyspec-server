//! The ports the translations domain depends on.

use std::path::{Path, PathBuf};
use std::rc::Rc;

use crate::values::{FileIdentity, RawTranslations, ReadError};

/// The two translation file formats WordPress reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileKind {
    /// A gettext `.mo` file.
    Mo,
    /// A `.l10n.php` file.
    Php,
}

impl FileKind {
    /// The kind a file name's extension names (`mo` or `php`), as `WP_Translation_File::create()` reads it.
    pub fn of_path(path: &Path) -> Option<FileKind> {
        let bytes = path.as_os_str().as_encoded_bytes();
        let ext = &bytes[bytes.iter().rposition(|b| *b == b'.')? + 1..];
        match ext {
            b"mo" => Some(FileKind::Mo),
            b"php" => Some(FileKind::Php),
            _ => None,
        }
    }
}

/// Reads a translation file into [`RawTranslations`].
pub trait TranslationFileReader {
    /// Reads `path` as the format its extension names.
    fn read(&self, path: &Path) -> Result<RawTranslations, ReadError>;

    /// Reads `path` as `kind`, whatever it is called (`WP_Translation_File::create( $file, $filetype )`). Readers
    /// that only know one format may keep the default, which reads by extension.
    fn read_as(&self, path: &Path, kind: FileKind) -> Result<RawTranslations, ReadError> {
        let _ = kind;
        self.read(path)
    }
}

/// The file system questions `WP_Translation_Controller` and `WP_Textdomain_Registry` ask.
pub trait FileProbe {
    /// `realpath()`.
    fn realpath(&self, path: &Path) -> Option<PathBuf>;
    /// `is_readable()`.
    fn is_readable(&self, path: &Path) -> bool;
    /// The file's identity, `None` when it does not exist.
    fn identity(&self, path: &Path) -> Option<FileIdentity>;
    /// `glob( "$dir$pattern" )`, where `pattern` may hold `*` (never `/`), sorted as `glob()` sorts.
    fn glob(&self, dir: &Path, pattern: &str) -> Vec<PathBuf>;
}

/// The loaded translation files a worker keeps across requests, one per identity.
pub trait TranslationFileCache<F> {
    fn get(&self, identity: &FileIdentity) -> Option<Rc<F>>;
    fn put(&self, identity: FileIdentity, file: Rc<F>);
}
