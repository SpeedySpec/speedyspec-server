//! WordPress translation file readers and the standard file probe: adapters of `speedyspec-wp-i18n-contracts`.

mod headers;
mod l10n_php;
mod mo;
pub mod po;
mod probe;

use std::path::Path;

use speedyspec_wp_i18n_contracts::{FileKind, RawTranslations, ReadError, TranslationFileReader};

pub use l10n_php::L10nPhpReader;
pub use mo::MoReader;
pub use probe::StdFileProbe;

/// Picks the reader by the text after the last `.`, as `WP_Translation_File::create()` does: `mo` or `php`.
#[derive(Clone, Copy, Debug, Default)]
pub struct ByExtension {
    mo: MoReader,
    php: L10nPhpReader,
}

impl ByExtension {
    /// Whether WordPress would create a reader for `path` at all.
    pub fn supports(path: &Path) -> bool {
        matches!(extension(path), Some(b"mo" | b"php"))
    }
}

fn extension(path: &Path) -> Option<&[u8]> {
    let bytes = path.as_os_str().as_encoded_bytes();
    bytes
        .iter()
        .rposition(|b| *b == b'.')
        .map(|i| &bytes[i + 1..])
}

impl TranslationFileReader for ByExtension {
    fn read(&self, path: &Path) -> Result<RawTranslations, ReadError> {
        match extension(path) {
            Some(b"mo") => self.mo.read(path),
            Some(b"php") => self.php.read(path),
            _ => Err(ReadError::Unreadable),
        }
    }

    fn read_as(&self, path: &Path, kind: FileKind) -> Result<RawTranslations, ReadError> {
        match kind {
            FileKind::Mo => self.mo.read(path),
            FileKind::Php => self.php.read(path),
        }
    }
}
