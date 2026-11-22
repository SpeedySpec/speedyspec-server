//! `sanitize_file_name()`'s core, split where its `sanitize_file_name_chars` filter runs.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::LazyLock;

use speedyspec_wp_utf8::is_valid_utf8;

use crate::adapters::mime_table::MIME_TYPES;
use crate::preg::{Pattern, preg_match, preg_replace};
use crate::strings::{StrReplace, pathinfo_extension, pathinfo_filename, php_trim};
use crate::title::{SanitizeContext, sanitize_title_with_dashes};

/// A MIME list as `wp_get_mime_types()` returns it: extension patterns (`jpg|jpeg|jpe`) and their types, in order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MimeTable(pub Vec<(Vec<u8>, Vec<u8>)>);

impl MimeTable {
    /// WordPress 7.1.5's `wp_get_mime_types()` before its `mime_types` filter.
    ///
    /// @return
    ///     The default list.
    pub fn wordpress() -> MimeTable {
        MimeTable(MIME_TYPES.iter().map(|(e, t)| (e.as_bytes().to_vec(), t.as_bytes().to_vec())).collect())
    }

    /// `get_allowed_mime_types()` for a user without `unfiltered_html`, before its `upload_mimes` filter: the
    /// default list less `swf`, `exe`, `htm|html` and `js`.
    ///
    /// @return
    ///     The allowed list.
    pub fn wordpress_allowed() -> MimeTable {
        let mut table = MimeTable::wordpress();
        table.0.retain(|(e, _)| !matches!(e.as_slice(), b"swf" | b"exe" | b"htm|html" | b"js"));
        table
    }
}

/// WordPress 7.1.5's `$special_chars` in `sanitize_file_name()`, in order, `chr( 0 )` last.
///
/// @return
///     The characters removed from file names, before the `sanitize_file_name_chars` filter.
pub fn default_special_chars() -> Vec<Vec<u8>> {
    let list: [&str; 32] = [
        "?", "[", "]", "/", "\\", "=", "<", ">", ":", ";", ",", "'", "\"", "&", "$", "#", "*", "(", ")", "|", "~",
        "`", "!", "{", "}", "%", "+", "’", "«", "»", "”", "“",
    ];
    let mut chars: Vec<Vec<u8>> = list.iter().map(|c| c.as_bytes().to_vec()).collect();
    chars.push(vec![0]);
    chars
}

/// `sanitize_file_name()`'s patterns.
struct FilePatterns {
    space_separator: Pattern,
    periods: Pattern,
    whitespace: Pattern,
    extension_like: Pattern,
}

static FILE: LazyLock<FilePatterns> = LazyLock::new(|| FilePatterns {
    space_separator: Pattern::php("#\\p{Zs}#siu"),
    periods: Pattern::php("/\\.{2,}/"),
    whitespace: Pattern::php("/[\\r\\n\\t -]+/"),
    extension_like: Pattern::php("/^[a-zA-Z]{2,5}\\d?$/"),
});

/// `str_replace( array( '%20', '+' ), '-', ... )`.
static SPACES: LazyLock<StrReplace> =
    LazyLock::new(|| StrReplace::new(vec![(b"%20".to_vec(), b"-".to_vec()), (b"+".to_vec(), b"-".to_vec())]));

/// `sanitize_file_name()` from after `remove_accents()` up to its `sanitize_file_name_chars` filter: names that
/// aren't UTF-8 are rebuilt from a slug of the name and the extension, and Unicode space separators become spaces.
///
/// @param name
///     The file name, after `remove_accents()`.
/// @return
///     The name the filtered special characters are removed from; `""` where PHP's `/u` replacement fails on
///     invalid UTF-8 and returns `null`.
pub fn sanitize_file_name_prepare(name: &[u8]) -> Vec<u8> {
    let name = if is_valid_utf8(name) {
        name.to_vec()
    } else {
        let extension = pathinfo_extension(name).unwrap_or_default().to_vec();
        let stem = pathinfo_filename(name);
        [sanitize_title_with_dashes(stem, &SanitizeContext::Display), b".".to_vec(), extension].concat()
    };
    preg_replace(&FILE.space_separator, &name, b" ", None).map_or_else(|_| Vec::new(), |(out, _)| out.into_owned())
}

thread_local! {
    /// Compiled extension patterns by source; `None` for one PCRE rejects.
    static EXTENSION_PATTERNS: RefCell<HashMap<Vec<u8>, Option<Rc<Pattern>>>> = RefCell::new(HashMap::new());
}

/// Whether `subject` matches the pattern built from `source`; a pattern PCRE rejects never matches (PHP warns and
/// returns `false`).
fn matches_built(source: Vec<u8>, subject: &[u8]) -> Option<Vec<u8>> {
    let pattern = EXTENSION_PATTERNS.with(|cache| {
        cache.borrow_mut().entry(source.clone()).or_insert_with(|| Pattern::try_php(&source).ok().map(Rc::new)).clone()
    })?;
    match preg_match(&pattern, subject) {
        Ok(Some(found)) => Some(found.get(1).unwrap_or_default().to_vec()),
        _ => None,
    }
}

/// `wp_check_filetype( $filename, $mimes )['ext']`.
fn check_filetype_ext(filename: &[u8], mimes: &MimeTable) -> Option<Vec<u8>> {
    mimes.0.iter().find_map(|(ext_preg, _)| matches_built([&b"!\\.("[..], ext_preg, b")$!i"].concat(), filename))
}

/// `sanitize_file_name()` after its `sanitize_file_name_chars` filter, up to (not including) its
/// `sanitize_file_name` filter.
///
/// @param name
///     What [`sanitize_file_name_prepare`] returned.
/// @param special_chars
///     The filtered list of characters to remove, in order.
/// @param mimes
///     `wp_get_mime_types()`.
/// @param allowed
///     `get_allowed_mime_types()`.
/// @return
///     The sanitized file name.
pub fn sanitize_file_name_core(name: &[u8], special_chars: &[Vec<u8>], mimes: &MimeTable, allowed: &MimeTable) -> Vec<u8> {
    let p = &*FILE;
    let remove = StrReplace::new(special_chars.iter().map(|c| (c.clone(), Vec::new())).collect());
    let name = remove.apply(name).into_owned();
    let name = SPACES.apply(&name).into_owned();
    let replace = |pattern: &Pattern, subject: &[u8], with: &[u8]| {
        preg_replace(pattern, subject, with, None).map_or_else(|_| Vec::new(), |(out, _)| out.into_owned())
    };
    let name = replace(&p.periods, &name, b".");
    let name = replace(&p.whitespace, &name, b"-");
    let mut name = php_trim(&name, b".-_").to_vec();
    if !name.contains(&b'.')
        && let Some(ext) = check_filetype_ext(&[b"test.", name.as_slice()].concat(), mimes)
        && ext == name
    {
        name = [&b"unnamed-file."[..], &ext].concat();
    }
    let parts: Vec<&[u8]> = name.split(|&b| b == b'.').collect();
    if parts.len() <= 2 {
        return name;
    }
    let mut out = parts[0].to_vec();
    for part in &parts[1..parts.len() - 1] {
        out.push(b'.');
        out.extend_from_slice(part);
        if matches!(preg_match(&p.extension_like, part), Ok(Some(_))) {
            let allowed_ext = allowed.0.iter().any(|(ext_preg, _)| {
                matches_built([&b"!^("[..], ext_preg, b")$!i"].concat(), part).is_some()
            });
            if !allowed_ext {
                out.push(b'_');
            }
        }
    }
    out.push(b'.');
    out.extend_from_slice(parts[parts.len() - 1]);
    out
}
