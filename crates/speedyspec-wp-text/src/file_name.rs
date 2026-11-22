//! `sanitize_file_name()`'s core, split where its `sanitize_file_name_chars` filter runs.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::LazyLock;

use speedyspec_wp_text_contracts::MimePairs;
use speedyspec_wp_utf8::is_valid_utf8;

use crate::adapters::mime_table::MIME_TYPES;
use crate::preg::{Pattern, preg_match, preg_replace};
use crate::sanitize::collapse_runs_to;
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
    extension_like: Pattern,
}

static FILE: LazyLock<FilePatterns> = LazyLock::new(|| FilePatterns {
    space_separator: Pattern::php("#\\p{Zs}#siu"),
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
    // In ASCII the only space separator is the space itself.
    if name.is_ascii() {
        return name;
    }
    preg_replace(&FILE.space_separator, &name, b" ", None).map_or_else(|_| Vec::new(), |(out, _)| out.into_owned())
}

/// Compiled patterns by MIME key; `None` for a key whose pattern PCRE rejects.
type PatternCache = RefCell<HashMap<Vec<u8>, Option<Rc<Pattern>>>>;

thread_local! {
    /// `wp_check_filetype()`'s `!\.(KEY)$!i`, compiled once per key; `None` for one PCRE rejects.
    static SUFFIX_PATTERNS: PatternCache = RefCell::new(HashMap::new());
    /// `sanitize_file_name()`'s `!^(KEY)$!i`, compiled once per key.
    static WHOLE_PATTERNS: PatternCache = RefCell::new(HashMap::new());
}

/// The pattern `prefix KEY suffix`, from `cache`; a pattern PCRE rejects never matches (PHP warns and returns
/// `false`).
fn built(
    cache: &'static std::thread::LocalKey<PatternCache>,
    key: &[u8],
    prefix: &[u8],
    suffix: &[u8],
) -> Option<Rc<Pattern>> {
    cache.with(|cache| {
        if let Some(found) = cache.borrow().get(key) {
            return found.clone();
        }
        let compiled = Pattern::try_php(&[prefix, key, suffix].concat()).ok().map(Rc::new);
        cache.borrow_mut().insert(key.to_vec(), compiled.clone());
        compiled
    })
}

/// Group 1 of `pattern` on `subject`, when it matches.
fn group_one(pattern: Option<Rc<Pattern>>, subject: &[u8]) -> Option<Vec<u8>> {
    let pattern = pattern?;
    match preg_match(&pattern, subject) {
        Ok(Some(found)) => Some(found.get(1).unwrap_or_default().to_vec()),
        _ => None,
    }
}

/// `wp_check_filetype( $filename, $mimes )['ext']`.
fn check_filetype_ext(filename: &[u8], mimes: &[(Vec<u8>, Vec<u8>)]) -> Option<Vec<u8>> {
    mimes.iter().find_map(|(ext_preg, _)| group_one(built(&SUFFIX_PATTERNS, ext_preg, b"!\\.(", b")$!i"), filename))
}

/// `sanitize_file_name()`'s default `$special_chars` as one prepared `str_replace()`.
///
/// @return
///     The replacement, built once.
pub fn default_special_chars_replace() -> &'static StrReplace {
    static DEFAULT: LazyLock<StrReplace> =
        LazyLock::new(|| StrReplace::new(default_special_chars().into_iter().map(|c| (c, Vec::new())).collect()));
    &DEFAULT
}

/// The `str_replace( $special_chars, '', ... )` for a filtered list.
///
/// @param special_chars
///     The characters to remove, in order.
/// @return
///     The prepared replacement.
pub fn special_chars_replace(special_chars: &[Vec<u8>]) -> StrReplace {
    StrReplace::new(special_chars.iter().map(|c| (c.clone(), Vec::new())).collect())
}

/// `sanitize_file_name()` after its `sanitize_file_name_chars` filter, up to (not including) its
/// `sanitize_file_name` filter.
///
/// @param name
///     What [`sanitize_file_name_prepare`] returned.
/// @param special
///     The filtered special characters, prepared ([`default_special_chars_replace`] when unfiltered).
/// @param mimes
///     `wp_get_mime_types()`, asked only when the name has no `.` left, as WordPress asks it.
/// @param allowed
///     `get_allowed_mime_types()`, asked only when the name has more than one `.`.
/// @return
///     The sanitized file name, or the first error the MIME lookups return.
pub fn sanitize_file_name_core<E>(
    name: &[u8],
    special: &StrReplace,
    mimes: impl FnOnce() -> Result<Rc<MimePairs>, E>,
    allowed: impl FnOnce() -> Result<Rc<MimePairs>, E>,
) -> Result<Vec<u8>, E> {
    let p = &*FILE;
    let name = special.apply(name).into_owned();
    let name = SPACES.apply(&name).into_owned();
    // `/\.{2,}/` to `.`, then `/[\r\n\t -]+/` to `-`.
    let name = collapse_runs_to(&name, |b| b == b'.', b'.');
    let name = collapse_runs_to(&name, |b| matches!(b, b'\r' | b'\n' | b'\t' | b' ' | b'-'), b'-');
    let mut name = php_trim(&name, b".-_").to_vec();
    if !name.contains(&b'.')
        && let Some(ext) = check_filetype_ext(&[b"test.", name.as_slice()].concat(), &mimes()?)
        && ext == name
    {
        name = [&b"unnamed-file."[..], &ext].concat();
    }
    let parts: Vec<&[u8]> = name.split(|&b| b == b'.').collect();
    if parts.len() <= 2 {
        return Ok(name);
    }
    let allowed = allowed()?;
    let mut out = parts[0].to_vec();
    for part in &parts[1..parts.len() - 1] {
        out.push(b'.');
        out.extend_from_slice(part);
        if matches!(preg_match(&p.extension_like, part), Ok(Some(_))) {
            let allowed_ext = allowed
                .iter()
                .any(|(ext_preg, _)| group_one(built(&WHOLE_PATTERNS, ext_preg, b"!^(", b")$!i"), part).is_some());
            if !allowed_ext {
                out.push(b'_');
            }
        }
    }
    out.push(b'.');
    out.extend_from_slice(parts[parts.len() - 1]);
    Ok(out)
}
