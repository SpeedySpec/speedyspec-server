//! The file reader the PHP side uses: the Rust readers, and PHP's `include` for a `.l10n.php` file outside the
//! grammar they parse, as WordPress itself loads every such file.

use std::path::Path;

use speedyspec_hooks::Value;
use speedyspec_wp_i18n_contracts::{RawEntry, RawTranslations, ReadError, TranslationFileReader};
use speedyspec_wp_i18n_files::ByExtension;

/// [`ByExtension`], with PHP's `include` behind [`ReadError::Unsupported`].
#[derive(Default)]
pub struct PhpFallbackReader {
    inner: ByExtension,
}

fn bytes_of(v: &Value) -> Option<Vec<u8>> {
    match v {
        Value::Str(s) => Some(s.as_bytes().to_vec()),
        Value::Bytes(b) => Some(b.clone()),
        Value::Int(n) => Some(n.to_string().into_bytes()),
        _ => None,
    }
}

impl TranslationFileReader for PhpFallbackReader {
    fn read(&self, path: &Path) -> Result<RawTranslations, ReadError> {
        match self.inner.read(path) {
            Err(ReadError::Unsupported) => {}
            other => return other,
        }
        let file = Value::Str(path.to_string_lossy().into_owned());
        let result = speedyspec_php::call(
            &Value::Str("SpeedySpec\\WpI18n\\include_translation_file".into()),
            &[file],
        )
        .map_err(|_| ReadError::Invalid("Invalid data"))?;
        let Value::Array(parts) = result else {
            return Err(ReadError::Invalid("Invalid data"));
        };
        let (Some((_, Value::Array(headers))), Some((_, Value::Array(entries)))) =
            (parts.first(), parts.get(1))
        else {
            return Err(ReadError::Invalid("Invalid data"));
        };
        let mut raw = RawTranslations::default();
        for (k, v) in headers {
            let name = match k {
                speedyspec_hooks::Key::Int(n) => n.to_string(),
                speedyspec_hooks::Key::Str(s) => s.clone(),
            };
            if let Some(value) = bytes_of(v) {
                raw.headers
                    .push((name, String::from_utf8_lossy(&value).into_owned()));
            }
        }
        for (k, v) in entries {
            let key = match k {
                speedyspec_hooks::Key::Int(n) => n.to_string().into_bytes(),
                speedyspec_hooks::Key::Str(s) => s.clone().into_bytes(),
            };
            let Some(value) = bytes_of(v) else { continue };
            let ks = raw.bytes.len() as u32;
            raw.bytes.extend_from_slice(&key);
            let vs = raw.bytes.len() as u32;
            raw.bytes.extend_from_slice(&value);
            raw.entries.push(RawEntry {
                key: ks..vs,
                value: vs..raw.bytes.len() as u32,
            });
        }
        Ok(raw)
    }
}
