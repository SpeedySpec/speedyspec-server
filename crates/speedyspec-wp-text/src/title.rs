//! `sanitize_title_with_dashes()` and `sanitize_user()`'s cores, and the context `sanitize_title()` passes on.

use std::sync::LazyLock;

use speedyspec_wp_utf8::is_valid_utf8;

pub use crate::accents::remove_accents;
use crate::helpers::utf8_uri_encode;
use crate::preg::{Pattern, preg_replace};
use crate::strings::{StrReplace, mb_strtolower, php_trim, replace_one, PHP_TRIM_DEFAULT};
use crate::strip_tags;

/// The `$context` WordPress passes to `sanitize_title()` and its filter.
///
/// WordPress compares it with `'save'`, `'query'` and `'display'`, and passes any other string through unchanged.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SanitizeContext {
    Display,
    Save,
    Query,
    Other(Vec<u8>),
}

impl SanitizeContext {
    /// The context for a string.
    ///
    /// @param name
    ///     The context string.
    /// @return
    ///     The named context, or `Other`.
    pub fn from_bytes(name: &[u8]) -> SanitizeContext {
        match name {
            b"display" => SanitizeContext::Display,
            b"save" => SanitizeContext::Save,
            b"query" => SanitizeContext::Query,
            other => SanitizeContext::Other(other.to_vec()),
        }
    }

    /// The context as WordPress's string.
    ///
    /// @return
    ///     `display`, `save`, `query` or the other string.
    pub fn as_bytes(&self) -> &[u8] {
        match self {
            SanitizeContext::Display => b"display",
            SanitizeContext::Save => b"save",
            SanitizeContext::Query => b"query",
            SanitizeContext::Other(name) => name,
        }
    }
}

/// `sanitize_title_with_dashes()`'s patterns, in the order its body uses them.
struct TitlePatterns {
    octet: Pattern,
    marked: Pattern,
    entity: Pattern,
    disallowed: Pattern,
    spaces: Pattern,
    dashes: Pattern,
}

static TITLE: LazyLock<TitlePatterns> = LazyLock::new(|| TitlePatterns {
    octet: Pattern::php("|%([a-fA-F0-9][a-fA-F0-9])|"),
    marked: Pattern::php("|---([a-fA-F0-9][a-fA-F0-9])---|"),
    entity: Pattern::php("/&.+?;/"),
    disallowed: Pattern::php("/[^%a-z0-9 _-]/"),
    spaces: Pattern::php("/\\s+/"),
    dashes: Pattern::php("|-+|"),
});

/// One `str_replace()` over a list of searches with one replacement.
fn replace_all(searches: &[&str], replace: &[u8]) -> StrReplace {
    StrReplace::new(searches.iter().map(|s| (s.as_bytes().to_vec(), replace.to_vec())).collect())
}

/// The `'save'` context's four `str_replace()` calls and its final one, in order.
static SAVE: LazyLock<[StrReplace; 5]> = LazyLock::new(|| {
    [
        replace_all(&["%c2%a0", "%e2%80%91", "%e2%80%93", "%e2%80%94"], b"-"),
        replace_all(&["&nbsp;", "&#8209;", "&#160;", "&ndash;", "&#8211;", "&mdash;", "&#8212;"], b"-"),
        replace_all(
            &[
                "%c2%ad", "%c2%a1", "%c2%bf", "%c2%ab", "%c2%bb", "%e2%80%b9", "%e2%80%ba", "%e2%80%98", "%e2%80%99",
                "%e2%80%9c", "%e2%80%9d", "%e2%80%9a", "%e2%80%9b", "%e2%80%9e", "%e2%80%9f", "%e2%80%a2", "%c2%a9",
                "%c2%ae", "%c2%b0", "%e2%80%a6", "%e2%84%a2", "%c2%b4", "%cb%8a", "%cc%81", "%cd%81", "%cc%80",
                "%cc%84", "%cc%8c", "%e2%80%8b", "%e2%80%8c", "%e2%80%8d", "%e2%80%8e", "%e2%80%8f", "%e2%80%aa",
                "%e2%80%ab", "%e2%80%ac", "%e2%80%ad", "%e2%80%ae", "%ef%bb%bf", "%ef%bf%bc",
            ],
            b"",
        ),
        replace_all(
            &[
                "%e2%80%80", "%e2%80%81", "%e2%80%82", "%e2%80%83", "%e2%80%84", "%e2%80%85", "%e2%80%86",
                "%e2%80%87", "%e2%80%88", "%e2%80%89", "%e2%80%8a", "%e2%80%a8", "%e2%80%a9", "%e2%80%af",
            ],
            b"-",
        ),
        replace_all(&["%c3%97"], b"x"),
    ]
});

/// `preg_replace()` with WordPress's own patterns; on a PCRE failure PHP returns `null`, which the next step reads
/// as `""`.
fn replace(pattern: &Pattern, subject: &[u8], replacement: &[u8]) -> Vec<u8> {
    preg_replace(pattern, subject, replacement, None).map_or_else(|_| Vec::new(), |(out, _)| out.into_owned())
}

/// WordPress's `sanitize_title_with_dashes()`: the slug WordPress makes from a title.
///
/// @param title
///     The title.
/// @param context
///     `Save` also turns dashes and spaces written as entities or UTF-8 into `-` and drops marks and invisibles.
/// @return
///     Lowercase letters, digits, `%XX` escapes, `_` and single dashes.
pub fn sanitize_title_with_dashes(title: &[u8], context: &SanitizeContext) -> Vec<u8> {
    let p = &*TITLE;
    let stripped = strip_tags(title);
    let title = replace(&p.octet, &stripped, b"---$1---");
    let title = replace_one(&title, b"%", b"").into_owned();
    let title = replace(&p.marked, &title, b"%$1");
    let title = if is_valid_utf8(&title) {
        let lower = mb_strtolower(std::str::from_utf8(&title).expect("validated"));
        utf8_uri_encode(lower.as_bytes(), 200, false)
    } else {
        title
    };
    let mut title = title.to_ascii_lowercase();
    if *context == SanitizeContext::Save {
        for step in SAVE.iter().take(2) {
            title = step.apply(&title).into_owned();
        }
        title = replace_one(&title, b"/", b"-").into_owned();
        for step in SAVE.iter().skip(2) {
            title = step.apply(&title).into_owned();
        }
    }
    let title = replace(&p.entity, &title, b"");
    let title = replace_one(&title, b".", b"-").into_owned();
    let title = replace(&p.disallowed, &title, b"");
    let title = replace(&p.spaces, &title, b"-");
    let title = replace(&p.dashes, &title, b"-");
    php_trim(&title, b"-").to_vec()
}

/// `sanitize_user()`'s patterns.
struct UserPatterns {
    octet: Pattern,
    entity: Pattern,
    strict: Pattern,
    spaces: Pattern,
}

static USER: LazyLock<UserPatterns> = LazyLock::new(|| UserPatterns {
    octet: Pattern::php("|%([a-fA-F0-9][a-fA-F0-9])|"),
    entity: Pattern::php("/&.+?;/"),
    strict: Pattern::php("|[^a-z0-9 _.\\-@]|i"),
    spaces: Pattern::php("|\\s+|"),
});

/// `sanitize_user()` before its filter.
///
/// @param username
///     The user name.
/// @param strict
///     Whether to keep only ASCII letters, digits, space and `_.-@`.
/// @param locale
///     The locale `remove_accents()` uses (WordPress's `get_locale()`).
/// @param strip_all_tags
///     `wp_strip_all_tags( $username )`.
/// @return
///     The sanitized user name.
pub fn sanitize_user_core(username: &[u8], strict: bool, locale: &str, strip_all_tags: impl Fn(&[u8]) -> Vec<u8>) -> Vec<u8> {
    let p = &*USER;
    let stripped = strip_all_tags(username);
    let name = remove_accents(&stripped, locale).into_owned();
    let name = replace(&p.octet, &name, b"");
    let mut name = replace(&p.entity, &name, b"");
    if strict {
        name = replace(&p.strict, &name, b"");
    }
    let trimmed = php_trim(&name, PHP_TRIM_DEFAULT);
    replace(&p.spaces, trimmed, b" ")
}
