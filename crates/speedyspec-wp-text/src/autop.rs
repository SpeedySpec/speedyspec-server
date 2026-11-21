//! `wpautop()` and `wp_replace_in_html_tags()`.

use std::borrow::Cow;
use std::sync::LazyLock;

use memchr::memmem;
use speedyspec_wp_html_split::wp_html_split;

use crate::preg::{Pattern, SplitFlags, preg_replace, preg_replace_callback, preg_split};
use crate::strings::{PHP_TRIM_DEFAULT, StrReplace, StrtrTable, php_trim, replace_one, strtr};

/// WordPress's `wp_replace_in_html_tags()`: replaces inside tags (and comments) only, never in text.
///
/// @param haystack
///     The HTML.
/// @param pairs
///     `(needle, replacement)` pairs. One pair uses `str_replace`; several use `strtr` on any tag that contains any
///     needle, as WordPress does.
/// @return
///     The result, borrowed when no tag changed.
pub fn wp_replace_in_html_tags<'a>(haystack: &'a [u8], pairs: &[(&[u8], &[u8])]) -> Cow<'a, [u8]> {
    let mut chunks: Vec<Cow<'a, [u8]>> = wp_html_split(haystack).into_iter().map(Cow::Borrowed).collect();
    let mut changed = false;
    if let [(needle, replace)] = pairs {
        for chunk in chunks.iter_mut().skip(1).step_by(2) {
            if memmem::find(chunk, needle).is_some() {
                *chunk = Cow::Owned(replace_one(chunk, needle, replace).into_owned());
                changed = true;
            }
        }
    } else {
        let table = StrtrTable::new(pairs.iter().map(|(a, b)| (a.to_vec(), b.to_vec())).collect());
        for chunk in chunks.iter_mut().skip(1).step_by(2) {
            if pairs.iter().any(|(needle, _)| memmem::find(chunk, needle).is_some()) {
                *chunk = Cow::Owned(strtr(chunk, &table).into_owned());
                changed = true;
            }
        }
    }
    if !changed {
        return Cow::Borrowed(haystack);
    }
    Cow::Owned(chunks.concat())
}

/// `wpautop()`'s options.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AutopOptions {
    /// Whether the remaining single line breaks become `<br />`.
    pub br: bool,
}

impl Default for AutopOptions {
    fn default() -> Self {
        AutopOptions { br: true }
    }
}

const ALLBLOCKS: &str = "(?:table|thead|tfoot|caption|col|colgroup|tbody|tr|td|th|div|dl|dd|dt|ul|ol|li|pre|form|map|area|blockquote|address|style|p|h[1-6]|hr|fieldset|legend|section|article|aside|hgroup|header|footer|nav|figure|figcaption|details|menu|summary)";

/// Every pattern in 7.1.5's `wpautop()`, in the order it uses them.
struct AutopPatterns {
    double_br: Pattern,
    block_open: Pattern,
    block_close: Pattern,
    hr: Pattern,
    option_before: Pattern,
    option_after: Pattern,
    object_open: Pattern,
    object_close: Pattern,
    param_embed: Pattern,
    media_open: Pattern,
    media_close: Pattern,
    source_track: Pattern,
    figcaption_open: Pattern,
    figcaption_close: Pattern,
    many_newlines: Pattern,
    paragraph_split: Pattern,
    empty_p: Pattern,
    p_unclosed_block: Pattern,
    p_around_block: Pattern,
    p_li: Pattern,
    p_blockquote: Pattern,
    p_before_block: Pattern,
    p_after_block: Pattern,
    preserve: Pattern,
    newline_br: Pattern,
    block_br: Pattern,
    br_before_block: Pattern,
    last_newline: Pattern,
}

static PATTERNS: LazyLock<AutopPatterns> = LazyLock::new(|| {
    let b = ALLBLOCKS;
    AutopPatterns {
        double_br: Pattern::php(r"|<br\s*/?>\s*<br\s*/?>|"),
        block_open: Pattern::php(&format!(r"!(<{b}[\s/>])!")),
        block_close: Pattern::php(&format!(r"!(</{b}>)!")),
        hr: Pattern::php(r"!(<hr\s*?/?>)!"),
        option_before: Pattern::php(r"|\s*<option|"),
        option_after: Pattern::php(r"|</option>\s*|"),
        object_open: Pattern::php(r"|(<object[^>]*>)\s*|"),
        object_close: Pattern::php(r"|\s*</object>|"),
        param_embed: Pattern::php(r"%\s*(</?(?:param|embed)[^>]*>)\s*%"),
        media_open: Pattern::php(r"%([<\[](?:audio|video)[^>\]]*[>\]])\s*%"),
        media_close: Pattern::php(r"%\s*([<\[]/(?:audio|video)[>\]])%"),
        source_track: Pattern::php(r"%\s*(<(?:source|track)[^>]*>)\s*%"),
        figcaption_open: Pattern::php(r"|\s*(<figcaption[^>]*>)|"),
        figcaption_close: Pattern::php(r"|</figcaption>\s*|"),
        many_newlines: Pattern::php(r"/\n\n+/"),
        paragraph_split: Pattern::php(r"/\n\s*\n/"),
        empty_p: Pattern::php(r"|<p>\s*</p>|"),
        p_unclosed_block: Pattern::php(r"!<p>([^<]+)</(div|address|form)>!"),
        p_around_block: Pattern::php(&format!(r"!<p>\s*(</?{b}[^>]*>)\s*</p>!")),
        p_li: Pattern::php(r"|<p>(<li.+?)</p>|"),
        p_blockquote: Pattern::php(r#"!<p><blockquote((?:[^>"']|"[^"]*"|'[^']*')*)>!i"#),
        p_before_block: Pattern::php(&format!(r"!<p>\s*(</?{b}[^>]*>)!")),
        p_after_block: Pattern::php(&format!(r"!(</?{b}[^>]*>)\s*</p>!")),
        preserve: Pattern::php(r"/<(script|style|svg|math).*?<\/\1>/s"),
        newline_br: Pattern::php(r"|(?<!<br />)\s*\n|"),
        block_br: Pattern::php(&format!(r"!(</?{b}[^>]*>)\s*<br />!")),
        br_before_block: Pattern::php(r"!<br />(\s*</?(?:p|li|div|dl|dd|dt|th|pre|td|ul|ol)[^>]*>)!"),
        last_newline: Pattern::php(r"|\n</p>$|"),
    }
});

/// `preg_replace()` as WordPress's code sees it: a failed match gives `null`, which the next string function
/// reads as `""`.
fn replace(pattern: &Pattern, text: Vec<u8>, replacement: &[u8]) -> Vec<u8> {
    match preg_replace(pattern, &text, replacement, None) {
        Ok((Cow::Owned(out), _)) => out,
        Ok((Cow::Borrowed(_), _)) => text,
        Err(_) => Vec::new(),
    }
}

fn contains(text: &[u8], needle: &[u8]) -> bool {
    memmem::find(text, needle).is_some()
}

static CRLF: LazyLock<StrReplace> =
    LazyLock::new(|| StrReplace::new(vec![(b"\r\n".to_vec(), b"\n".to_vec()), (b"\r".to_vec(), b"\n".to_vec())]));
static BR_FORMS: LazyLock<StrReplace> = LazyLock::new(|| {
    StrReplace::new(vec![(b"<br>".to_vec(), b"<br />".to_vec()), (b"<br/>".to_vec(), b"<br />".to_vec())])
});
static WPNL: LazyLock<StrReplace> = LazyLock::new(|| {
    StrReplace::new(vec![(b" <!-- wpnl --> ".to_vec(), b"\n".to_vec()), (b"<!-- wpnl -->".to_vec(), b"\n".to_vec())])
});

/// WordPress's `wpautop()`, transcribed pass for pass on PCRE2: the reference the faster `wpautop` is checked
/// against.
///
/// @param text
///     The text.
/// @param options
///     Whether single line breaks become `<br />`.
/// @return
///     The text with paragraphs; whitespace-only input gives `""`.
pub fn wpautop_reference(text: &[u8], options: AutopOptions) -> Cow<'_, [u8]> {
    if php_trim(text, PHP_TRIM_DEFAULT).is_empty() {
        return Cow::Borrowed(&text[..0]);
    }
    let p = &*PATTERNS;
    let mut text = [text, b"\n"].concat();
    let mut pre_tags: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();

    // Pre tags shouldn't be touched by autop; replace them with placeholders and bring them back after.
    if contains(&text, b"<pre") {
        let mut parts: Vec<&[u8]> = Vec::new();
        let mut rest = &text[..];
        while let Some(at) = memmem::find(rest, b"</pre>") {
            parts.push(&rest[..at]);
            rest = &rest[at + 6..];
        }
        let last = rest;
        let mut rebuilt = Vec::with_capacity(text.len());
        let mut i = 0;
        for part in parts {
            let Some(start) = memmem::find(part, b"<pre") else {
                rebuilt.extend_from_slice(part);
                continue;
            };
            let name = format!("<pre wp-pre-tag-{i}></pre>").into_bytes();
            pre_tags.push((name.clone(), [&part[start..], b"</pre>"].concat()));
            rebuilt.extend_from_slice(&part[..start]);
            rebuilt.extend_from_slice(&name);
            i += 1;
        }
        rebuilt.extend_from_slice(last);
        text = rebuilt;
    }

    text = replace(&p.double_br, text, b"\n\n");
    text = replace(&p.block_open, text, b"\n\n$1");
    text = replace(&p.block_close, text, b"$1\n\n");
    text = replace(&p.hr, text, b"$1\n\n");
    text = CRLF.apply(&text).into_owned();
    text = wp_replace_in_html_tags(&text, &[(b"\n", b" <!-- wpnl --> ")]).into_owned();

    if contains(&text, b"<option") {
        text = replace(&p.option_before, text, b"<option");
        text = replace(&p.option_after, text, b"</option>");
    }
    if contains(&text, b"</object>") {
        text = replace(&p.object_open, text, b"$1");
        text = replace(&p.object_close, text, b"</object>");
        text = replace(&p.param_embed, text, b"$1");
    }
    if contains(&text, b"<source") || contains(&text, b"<track") {
        text = replace(&p.media_open, text, b"$1");
        text = replace(&p.media_close, text, b"$1");
        text = replace(&p.source_track, text, b"$1");
    }
    if contains(&text, b"<figcaption") {
        text = replace(&p.figcaption_open, text, b"$1");
        text = replace(&p.figcaption_close, text, b"</figcaption>");
    }

    text = replace(&p.many_newlines, text, b"\n\n");
    let flags = SplitFlags { delim_capture: false, no_empty: true };
    let paragraphs = preg_split(&p.paragraph_split, &text, -1, flags).unwrap_or_default();
    let mut joined = Vec::with_capacity(text.len() + paragraphs.len() * 8);
    for paragraph in paragraphs {
        joined.extend_from_slice(b"<p>");
        joined.extend_from_slice(php_trim(paragraph, b"\n"));
        joined.extend_from_slice(b"</p>\n");
    }
    text = joined;

    text = replace(&p.empty_p, text, b"");
    text = replace(&p.p_unclosed_block, text, b"<p>$1</p></$2>");
    text = replace(&p.p_around_block, text, b"$1");
    text = replace(&p.p_li, text, b"$1");
    text = replace(&p.p_blockquote, text, b"<blockquote$1><p>");
    text = replace_one(&text, b"</blockquote></p>", b"</p></blockquote>").into_owned();
    text = replace(&p.p_before_block, text, b"$1");
    text = replace(&p.p_after_block, text, b"$1");

    if options.br {
        text = match preg_replace_callback(&p.preserve, &text, |m| {
            replace_one(m.full(), b"\n", b"<WPPreserveNewline />").into_owned()
        }) {
            Ok(Cow::Owned(out)) => out,
            Ok(Cow::Borrowed(_)) => text,
            Err(_) => Vec::new(),
        };
        text = BR_FORMS.apply(&text).into_owned();
        text = replace(&p.newline_br, text, b"<br />\n");
        text = replace_one(&text, b"<WPPreserveNewline />", b"\n").into_owned();
    }

    text = replace(&p.block_br, text, b"$1");
    text = replace(&p.br_before_block, text, b"$1");
    text = replace(&p.last_newline, text, b"</p>");

    if !pre_tags.is_empty() {
        text = StrReplace::new(pre_tags).apply(&text).into_owned();
    }
    if contains(&text, b"<!-- wpnl -->") {
        text = WPNL.apply(&text).into_owned();
    }
    Cow::Owned(text)
}
