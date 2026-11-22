//! Smileys: `smilies_init()`'s search pattern, `translate_smiley()` and `convert_smilies()`.

use std::borrow::Cow;
use std::sync::LazyLock;

use speedyspec_wp_shortcode_contracts::preg_quote;

use crate::preg::{Pattern, SplitFlags, preg_match, preg_replace_callback, preg_split};
use crate::small::DEFAULT_SPACES;

/// `preg_quote( $text, '/' )`.
fn quote(text: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len() * 2);
    for chunk in preg_quote(text).split_inclusive(|&b| b == b'/') {
        match chunk.split_last() {
            Some((b'/', head)) => {
                out.extend_from_slice(head);
                out.extend_from_slice(b"\\/");
            }
            _ => out.extend_from_slice(chunk),
        }
    }
    out
}

/// The search pattern `smilies_init()` builds into `$wp_smiliessearch`.
///
/// @param table
///     `$wpsmiliestrans` after its `smilies` filter: `(smiley, image)` pairs.
/// @param spaces
///     `wp_spaces_regexp()`.
/// @return
///     The pattern, or `None` when the table is empty (WordPress leaves `$wp_smiliessearch` unset).
pub fn smilies_search_pattern(table: &[(Vec<u8>, Vec<u8>)], spaces: &[u8]) -> Option<Vec<u8>> {
    if table.is_empty() {
        return None;
    }
    // krsort(): keys in descending byte order.
    let mut smileys: Vec<&[u8]> = table.iter().map(|(s, _)| s.as_slice()).collect();
    smileys.sort_unstable_by(|a, b| b.cmp(a));
    let mut out = [b"/(?<=", spaces, b"|^)"].concat();
    let mut subchar: Option<&[u8]> = None;
    for smiley in smileys {
        let (first, rest) = smiley.split_at(smiley.len().min(1));
        if subchar != Some(first) {
            if subchar.is_some() {
                out.extend_from_slice(&[b")(?=", spaces, b"|$)"].concat());
                out.extend_from_slice(&[b"|(?<=", spaces, b"|^)"].concat());
            }
            subchar = Some(first);
            out.extend_from_slice(&quote(first));
            out.extend_from_slice(b"(?:");
        } else {
            out.push(b'|');
        }
        out.extend_from_slice(&quote(rest));
    }
    out.extend_from_slice(&[b")(?=", spaces, b"|$)/m"].concat());
    Some(out)
}


/// `convert_smilies()`'s patterns.
static SMILEY_SPLIT: LazyLock<(Pattern, Pattern)> =
    LazyLock::new(|| (Pattern::php("/(<[^>]*>)/U"), Pattern::php("/^<(code|pre|style|script|textarea)[^>]*>/")));

/// The smileys for one first byte, in the search's (`krsort()`) order, without that byte.
type Group = Vec<Vec<u8>>;

/// A matcher equivalent to the search pattern `smilies_init()` builds with the default spaces: a smiley preceded by
/// the start, a space, `\xC2\xA0` or `&nbsp;`, and followed by one or the end; at each position the first smiley
/// in descending order that fits wins, as PCRE's alternation does.
pub struct HandSmilies {
    groups: Vec<Option<Group>>,
}

impl HandSmilies {
    fn new(smileys: &[Vec<u8>]) -> HandSmilies {
        let mut sorted: Vec<&Vec<u8>> = smileys.iter().filter(|s| !s.is_empty()).collect();
        sorted.sort_unstable_by(|a, b| b.cmp(a));
        let mut groups: Vec<Option<Group>> = vec![None; 256];
        for smiley in sorted {
            groups[usize::from(smiley[0])].get_or_insert_with(Vec::new).push(smiley[1..].to_vec());
        }
        HandSmilies { groups }
    }

    /// Whether a space fragment ends at `at` (or `at` is the start).
    fn space_before(text: &[u8], at: usize) -> bool {
        at == 0
            || matches!(text[at - 1], b'\r' | b'\n' | b'\t' | b' ')
            || text[..at].ends_with(b"\xC2\xA0")
            || text[..at].ends_with(b"&nbsp;")
    }

    /// Whether a space fragment starts at `at` (or `at` is the end).
    fn space_after(text: &[u8], at: usize) -> bool {
        at == text.len()
            || matches!(text[at], b'\r' | b'\n' | b'\t' | b' ')
            || text[at..].starts_with(b"\xC2\xA0")
            || text[at..].starts_with(b"&nbsp;")
    }

    /// The length of the smiley matched at `at`, if any.
    fn at(&self, text: &[u8], at: usize) -> Option<usize> {
        let group = self.groups[usize::from(text[at])].as_ref()?;
        if !HandSmilies::space_before(text, at) {
            return None;
        }
        group
            .iter()
            .find(|rest| text[at + 1..].starts_with(rest) && HandSmilies::space_after(text, at + 1 + rest.len()))
            .map(|rest| 1 + rest.len())
    }

    /// `preg_replace_callback( $wp_smiliessearch, ..., $text )`.
    fn replace<'a>(&self, text: &'a [u8], translate: &mut dyn FnMut(&[u8]) -> Vec<u8>) -> Cow<'a, [u8]> {
        let mut out: Option<Vec<u8>> = None;
        let mut copied = 0;
        let mut i = 0;
        while i < text.len() {
            match self.at(text, i) {
                Some(len) => {
                    let buf = out.get_or_insert_with(|| Vec::with_capacity(text.len() + 64));
                    buf.extend_from_slice(&text[copied..i]);
                    buf.extend_from_slice(&translate(&text[i..i + len]));
                    i += len;
                    copied = i;
                }
                None => i += 1,
            }
        }
        match out {
            None => Cow::Borrowed(text),
            Some(mut buf) => {
                buf.extend_from_slice(&text[copied..]);
                Cow::Owned(buf)
            }
        }
    }
}

/// How smileys are found: the hand matcher when it is equivalent, WordPress's pattern otherwise.
pub enum SmileyEngine {
    Hand(HandSmilies),
    Pattern(Pattern),
}

impl SmileyEngine {
    /// The engine for a search.
    ///
    /// @param search
    ///     `$wp_smiliessearch`.
    /// @param smileys
    ///     The keys of `$wpsmiliestrans`, in order.
    /// @return
    ///     The hand matcher when `search` is exactly what `smilies_init()` builds from `smileys` with the default
    ///     spaces, else the compiled pattern; `None` when PCRE rejects the search.
    pub fn new(search: &[u8], smileys: &[Vec<u8>]) -> Option<SmileyEngine> {
        let table: Vec<(Vec<u8>, Vec<u8>)> = smileys.iter().map(|s| (s.clone(), Vec::new())).collect();
        if smilies_search_pattern(&table, DEFAULT_SPACES).as_deref() == Some(search) {
            return Some(SmileyEngine::Hand(HandSmilies::new(smileys)));
        }
        SmileyEngine::reference(search)
    }

    /// The pattern engine, always.
    ///
    /// @param search
    ///     `$wp_smiliessearch`.
    /// @return
    ///     The compiled pattern, or `None` when PCRE rejects it.
    pub fn reference(search: &[u8]) -> Option<SmileyEngine> {
        Pattern::try_php(search).ok().map(SmileyEngine::Pattern)
    }

    /// Whether this is the hand matcher.
    pub fn is_hand(&self) -> bool {
        matches!(self, SmileyEngine::Hand(_))
    }

    fn replace<'a>(&self, text: &'a [u8], translate: &mut dyn FnMut(&[u8]) -> Vec<u8>) -> Cow<'a, [u8]> {
        match self {
            SmileyEngine::Hand(hand) => hand.replace(text, translate),
            SmileyEngine::Pattern(pattern) => {
                // PHP's preg_replace_callback returns null on failure, which the concatenation reads as "".
                preg_replace_callback(pattern, text, |m| translate(m.get(0).unwrap_or_default()))
                    .unwrap_or(Cow::Borrowed(&text[..0]))
            }
        }
    }
}

/// `convert_smilies()` once smileys are on and the search is set, splitting with WordPress's patterns: the oracle for
/// [`convert_smilies_core`].
///
/// @param text
///     The text.
/// @param engine
///     The matcher.
/// @param translate
///     `translate_smiley()`, given each matched smiley.
/// @return
///     The text with smileys replaced outside tags and outside `code`, `pre`, `style`, `script` and `textarea`.
pub fn convert_smilies_reference<'a>(text: &'a [u8], engine: &SmileyEngine, translate: &mut dyn FnMut(&[u8]) -> Vec<u8>) -> Cow<'a, [u8]> {
    let (split, ignore_open) = &*SMILEY_SPLIT;
    let Ok(pieces) = preg_split(split, text, -1, SplitFlags { delim_capture: true, no_empty: false }) else {
        return Cow::Borrowed(text);
    };
    let mut out: Vec<u8> = Vec::with_capacity(text.len() + 64);
    let mut changed = false;
    let mut ignore: Vec<u8> = Vec::new();
    for content in pieces {
        if ignore.is_empty()
            && let Ok(Some(m)) = preg_match(ignore_open, content)
        {
            ignore = m.get(1).unwrap_or_default().to_vec();
        }
        let mut piece = Cow::Borrowed(content);
        if ignore.is_empty() && !content.is_empty() && content[0] != b'<' {
            piece = engine.replace(content, translate);
            changed |= matches!(piece, Cow::Owned(_));
        }
        if !ignore.is_empty() && content.len() == ignore.len() + 3 && content.starts_with(b"</") && content.ends_with(b">")
            && content[2..content.len() - 1] == ignore[..]
        {
            ignore.clear();
        }
        out.extend_from_slice(&piece);
    }
    if changed || out != text { Cow::Owned(out) } else { Cow::Borrowed(text) }
}

/// The tags `convert_smilies()` skips the content of.
const IGNORED: [&[u8]; 5] = [b"code", b"pre", b"style", b"script", b"textarea"];

/// `preg_match( '/^<(code|pre|style|script|textarea)[^>]*>/', $content )`'s group 1 on a piece.
fn ignored_open(piece: &[u8]) -> Option<&'static [u8]> {
    if piece.first() != Some(&b'<') || !piece.contains(&b'>') {
        return None;
    }
    IGNORED.iter().copied().find(|name| piece[1..].starts_with(name))
}

/// `convert_smilies()` once smileys are on and the search is set.
///
/// The same bytes as [`convert_smilies_reference`], splitting on `<...>` by hand: `/(<[^>]*>)/U` is a `<`, then
/// everything up to the next `>`.
///
/// @param text
///     The text.
/// @param engine
///     The matcher.
/// @param translate
///     `translate_smiley()`, given each matched smiley.
/// @return
///     The text with smileys replaced outside tags and outside `code`, `pre`, `style`, `script` and `textarea`;
///     borrowed when nothing changed.
pub fn convert_smilies_core<'a>(text: &'a [u8], engine: &SmileyEngine, translate: &mut dyn FnMut(&[u8]) -> Vec<u8>) -> Cow<'a, [u8]> {
    let mut out: Option<Vec<u8>> = None;
    let mut ignore: Option<&'static [u8]> = None;
    let mut at = 0;
    let mut text_piece = |piece: &[u8], at_piece: usize, ignore: Option<&'static [u8]>, out: &mut Option<Vec<u8>>| {
        // `strlen( $content ) > 0 && '<' !== $content[0]`: a `<` with no `>` after it starts a text piece that is
        // left alone too.
        if ignore.is_none()
            && piece.first().is_some_and(|&b| b != b'<')
            && let Cow::Owned(changed) = engine.replace(piece, translate)
        {
            let buf = out.get_or_insert_with(|| text[..at_piece].to_vec());
            buf.extend_from_slice(&changed);
            return;
        }
        if let Some(buf) = out.as_mut() {
            buf.extend_from_slice(piece);
        }
    };
    while at < text.len() {
        let tag = memchr::memchr(b'<', &text[at..]).and_then(|lt| {
            let lt = at + lt;
            memchr::memchr(b'>', &text[lt..]).map(|gt| (lt, lt + gt + 1))
        });
        let Some((lt, end)) = tag else {
            // The rest is one text piece (a `<` with no `>` after it never opens an ignored tag).
            text_piece(&text[at..], at, ignore, &mut out);
            break;
        };
        text_piece(&text[at..lt], at, ignore, &mut out);
        let tag = &text[lt..end];
        if ignore.is_none() {
            ignore = ignored_open(tag);
        }
        if let Some(name) = ignore
            && tag.len() == name.len() + 3
            && tag.starts_with(b"</")
            && &tag[2..tag.len() - 1] == name
        {
            ignore = None;
        }
        if let Some(buf) = out.as_mut() {
            buf.extend_from_slice(tag);
        }
        at = end;
    }
    match out {
        Some(buf) => Cow::Owned(buf),
        None => Cow::Borrowed(text),
    }
}
