//! `WP_Translation_File_MO::parse_file()`, reading the whole file into owned memory.

use std::path::Path;

use speedyspec_wp_i18n_contracts::{RawEntry, RawTranslations, ReadError, TranslationFileReader};
use speedyspec_wp_utf8::php_substr;

use crate::headers::{assign, php_trim, text};

const MAGIC: u32 = 0x9504_12de;

/// Reads `.mo` files as WordPress does, both byte orders.
#[derive(Clone, Copy, Debug, Default)]
pub struct MoReader;

/// The byte range `php_substr( bytes, start, length )` returns, as offsets into `bytes`.
fn substr_range(bytes: &[u8], start: i64, length: i64) -> (usize, usize) {
    let s = php_substr(bytes, start, Some(length));
    if s.is_empty() {
        return (0, 0);
    }
    let from = s.as_ptr() as usize - bytes.as_ptr() as usize;
    (from, from + s.len())
}

impl MoReader {
    /// Parses MO bytes with WordPress's rules.
    pub fn parse(bytes: Vec<u8>) -> Result<RawTranslations, ReadError> {
        let len = bytes.len();
        if len < 24 {
            return Err(ReadError::Invalid("Invalid data"));
        }
        let word = |at: usize, big: bool| -> Option<u32> {
            let b: [u8; 4] = bytes.get(at..at + 4)?.try_into().ok()?;
            Some(if big {
                u32::from_be_bytes(b)
            } else {
                u32::from_le_bytes(b)
            })
        };
        let big = if word(0, true) == Some(MAGIC) {
            true
        } else if word(0, false) == Some(MAGIC) {
            false
        } else {
            return Err(ReadError::Invalid("Magic marker does not exist"));
        };
        // `unpack()` of the six offsets fails (and WordPress stops, without an error) when bytes 4..28 are short.
        let Some(
            [
                rev,
                _total,
                originals_addr,
                translations_addr,
                _hash_length,
                hash_addr,
            ],
        ) = (|| {
            Some([
                word(4, big)?,
                word(8, big)?,
                word(12, big)?,
                word(16, big)?,
                word(20, big)?,
                word(24, big)?,
            ])
        })()
        else {
            return Ok(RawTranslations {
                bytes,
                ..RawTranslations::default()
            });
        };
        if rev > 0 {
            return Err(ReadError::Invalid("Unsupported revision"));
        }
        if translations_addr as usize > len || originals_addr as usize > len {
            return Err(ReadError::Invalid("Invalid data"));
        }
        let originals_length = i64::from(translations_addr) - i64::from(originals_addr);
        let translations_length = i64::from(hash_addr) - i64::from(translations_addr);
        let (o_from, o_to) = substr_range(&bytes, i64::from(originals_addr), originals_length);
        let (t_from, t_to) =
            substr_range(&bytes, i64::from(translations_addr), translations_length);
        let pair = |at: usize, end: usize| -> Option<(u32, u32)> {
            if at + 8 > end {
                return None;
            }
            Some((word(at, big)?, word(at + 4, big)?))
        };

        let mut raw = RawTranslations {
            bytes: Vec::new(),
            headers: Vec::new(),
            entries: Vec::new(),
        };
        let chunks = (o_to - o_from).div_ceil(8);
        for i in 0..chunks {
            let (Some((o_len, o_pos)), Some((t_len, t_pos))) =
                (pair(o_from + 8 * i, o_to), pair(t_from + 8 * i, t_to))
            else {
                continue;
            };
            let (os, oe) = substr_range(&bytes, i64::from(o_pos), i64::from(o_len));
            let (ts, mut te) = substr_range(&bytes, i64::from(t_pos), i64::from(t_len));
            // GlotPress bug: trailing NULs on translations.
            while te > ts && bytes[te - 1] == 0 {
                te -= 1;
            }
            if os == oe {
                for line in bytes[ts..te].split(|b| *b == b'\n') {
                    let Some(colon) = line.iter().position(|b| *b == b':') else {
                        continue;
                    };
                    let name = php_trim(&line[..colon]).to_ascii_lowercase();
                    assign(
                        &mut raw.headers,
                        text(&name),
                        text(php_trim(&line[colon + 1..])),
                    );
                }
            } else {
                let key_end = bytes[os..oe]
                    .iter()
                    .position(|b| *b == 0)
                    .map_or(oe, |i| os + i);
                raw.entries.push(RawEntry {
                    key: os as u32..key_end as u32,
                    value: ts as u32..te as u32,
                });
            }
        }
        raw.bytes = bytes;
        Ok(raw)
    }
}

impl TranslationFileReader for MoReader {
    fn read(&self, path: &Path) -> Result<RawTranslations, ReadError> {
        let bytes = std::fs::read(path).map_err(|_| ReadError::Unreadable)?;
        MoReader::parse(bytes)
    }
}
