//! The ports the translations domain depends on.

use std::path::{Path, PathBuf};
use std::rc::Rc;

use crate::values::{FileIdentity, RawTranslations, ReadError};

/// Reads a translation file into [`RawTranslations`].
pub trait TranslationFileReader {
    fn read(&self, path: &Path) -> Result<RawTranslations, ReadError>;
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
