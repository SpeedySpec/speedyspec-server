//! `wpautop()` with hand-written passes.
//!
//! The same passes as [`wpautop_reference`], in the same order, each giving its pattern's exact matches; the hot
//! ones are scanners over `memchr`/`memmem` candidates, and a pass whose trigger text is absent is skipped. The
//! rare, attribute-aware passes (`<blockquote` attributes, `<script>`/`<style>` newline preservation, `<option>`,
//! `<object>`, media and `<figcaption>` cleanup) still run WordPress's patterns. `tests/differential.rs` compares
//! the two.

use std::borrow::Cow;
use std::sync::LazyLock;

use memchr::memmem;

use crate::autop::{AutopOptions, PATTERNS};
use crate::preg::{Pattern, preg_replace, preg_replace_callback};
use crate::scan::{pass, pass_needle, skip_spaces};
use crate::strings::{PHP_TRIM_DEFAULT, StrReplace, php_trim, replace_one};

/// `wpautop()`'s `$allblocks` names.
const BLOCKS: &[&[u8]] = &[
    b"table", b"thead", b"tfoot", b"caption", b"col", b"colgroup", b"tbody", b"tr", b"td", b"th", b"div", b"dl",
    b"dd", b"dt", b"ul", b"ol", b"li", b"pre", b"form", b"map", b"area", b"blockquote", b"address", b"style", b"p",
    b"h1", b"h2", b"h3", b"h4", b"h5", b"h6", b"hr", b"fieldset", b"legend", b"section", b"article", b"aside",
    b"hgroup", b"header", b"footer", b"nav", b"figure", b"figcaption", b"details", b"menu", b"summary",
];

/// `(?:BLOCK)[\s/>]` or `(?:BLOCK)>` at `p`: the name must be exactly one of the blocks, because the byte after it
/// can't continue a name. Returns where the name ends.
fn exact_block(t: &[u8], p: usize) -> Option<usize> {
    let end = p + t.get(p..)?.iter().take_while(|b| b.is_ascii_lowercase() || b.is_ascii_digit()).count();
    let name = &t[p..end];
    let first = *name.first()?;
    BLOCKS.iter().any(|b| b.len() == name.len() && b[0] == first && *b == name).then_some(end)
}

/// Whether `rest` starts with one of `$allblocks`' names.
fn starts_with_block(rest: &[u8]) -> bool {
    let any = |names: &[&[u8]]| names.iter().any(|n| rest.starts_with(n));
    match rest.first() {
        Some(b'p') => true,
        Some(b't') => any(&[b"table", b"thead", b"tfoot", b"tbody", b"tr", b"td", b"th"]),
        Some(b'c') => any(&[b"caption", b"col"]),
        Some(b'd') => any(&[b"div", b"dl", b"dd", b"dt", b"details"]),
        Some(b'u') => any(&[b"ul"]),
        Some(b'o') => any(&[b"ol"]),
        Some(b'l') => any(&[b"li", b"legend"]),
        Some(b'f') => any(&[b"form", b"fieldset", b"footer", b"figure", b"figcaption"]),
        Some(b'm') => any(&[b"map", b"menu"]),
        Some(b'a') => any(&[b"area", b"address", b"article", b"aside"]),
        Some(b'b') => any(&[b"blockquote"]),
        Some(b's') => any(&[b"style", b"section", b"summary"]),
        Some(b'h') => matches!(rest.get(1), Some(b'1'..=b'6' | b'r')) || any(&[b"hgroup", b"header"]),
        Some(b'n') => any(&[b"nav"]),
        _ => false,
    }
}

/// Whether `rest` starts with one of `p|li|div|dl|dd|dt|th|pre|td|ul|ol`.
fn starts_with_br_block(rest: &[u8]) -> bool {
    match rest.first() {
        Some(b'p') => true,
        Some(b'l') => rest.get(1) == Some(&b'i'),
        Some(b'd') => matches!(rest.get(1), Some(b'l' | b'd' | b't')) || rest.starts_with(b"div"),
        Some(b't') => matches!(rest.get(1), Some(b'h' | b'd')),
        Some(b'u' | b'o') => rest.get(1) == Some(&b'l'),
        _ => false,
    }
}

/// `</?(?:NAMES)[^>]*>` at `p` (a `<`): any name may start the tag, because `[^>]*` takes the rest. Returns where
/// the tag ends.
fn block_tag(t: &[u8], p: usize, names: fn(&[u8]) -> bool) -> Option<usize> {
    if t.get(p) != Some(&b'<') {
        return None;
    }
    let mut j = p + 1;
    if t.get(j) == Some(&b'/') {
        j += 1;
    }
    let rest = t.get(j..)?;
    if !names(rest) {
        return None;
    }
    memchr::memchr(b'>', rest).map(|at| j + at + 1)
}

fn contains(t: &[u8], needle: &[u8]) -> bool {
    memmem::find(t, needle).is_some()
}

/// Runs a pass on an owned text, keeping it when nothing changed.
fn step(text: Vec<u8>, run: impl FnOnce(&[u8]) -> Cow<'_, [u8]>) -> Vec<u8> {
    match run(&text) {
        Cow::Borrowed(_) => text,
        Cow::Owned(changed) => changed,
    }
}

/// A WordPress pattern on an owned text, as `wpautop_reference` runs it.
fn pattern(text: Vec<u8>, pattern: &Pattern, replacement: &[u8]) -> Vec<u8> {
    match preg_replace(pattern, &text, replacement, None) {
        Ok((Cow::Owned(out), _)) => out,
        Ok((Cow::Borrowed(_), _)) => text,
        Err(_) => Vec::new(),
    }
}

static CRLF: LazyLock<StrReplace> =
    LazyLock::new(|| StrReplace::new(vec![(b"\r\n".to_vec(), b"\n".to_vec()), (b"\r".to_vec(), b"\n".to_vec())]));
static WPNL: LazyLock<StrReplace> = LazyLock::new(|| {
    StrReplace::new(vec![(b" <!-- wpnl --> ".to_vec(), b"\n".to_vec()), (b"<!-- wpnl -->".to_vec(), b"\n".to_vec())])
});
/// Which of `needles` occur in `text`, as bits. Every needle but `"\r"` starts with `<`, so the scan visits only
/// the `<` bytes, and compares only the needles whose second byte matches.
fn presence(needles: &[&[u8]], text: &[u8]) -> u32 {
    let all = (1u32 << needles.len()) - 1;
    let mut found = 0u32;
    let mut by_second: [u32; 256] = [0; 256];
    for (n, needle) in needles.iter().enumerate() {
        if needle[0] != b'<' {
            if memchr::memchr(needle[0], text).is_some() {
                found |= 1 << n;
            }
        } else {
            by_second[usize::from(needle[1])] |= 1 << n;
        }
    }
    for i in memchr::memchr_iter(b'<', text) {
        let Some(&second) = text.get(i + 1) else { continue };
        let mut candidates = by_second[usize::from(second)] & !found;
        while candidates != 0 {
            let n = candidates.trailing_zeros() as usize;
            candidates &= candidates - 1;
            if text[i..].starts_with(needles[n]) {
                found |= 1 << n;
            }
        }
        if found == all {
            break;
        }
    }
    found
}

/// Texts whose absence from the input means a pass can't match anywhere later: none of them is created by an
/// earlier pass, so checking the input once is checking a superset.
const EARLY: [&[u8]; 12] =
    [b"<pre", b"<br", b"<hr", b"\r", b"<script", b"<style", b"<svg", b"<math", b"<li", b"</div>", b"</address>", b"</form>"];
const PRE: u32 = 1;
const BR: u32 = 1 << 1;
const HR: u32 = 1 << 2;
const CR: u32 = 1 << 3;
const PRESERVE: u32 = 0b1111 << 4;
const LI: u32 = 1 << 8;
const UNCLOSED: u32 = 0b111 << 9;

/// WordPress's own guards, all checked at the same point on the same text: none of their passes adds or removes
/// another's text.
const GUARDS: [&[u8]; 5] = [b"<option", b"</object>", b"<source", b"<track", b"<figcaption"];

/// `<p><blockquote`, in any case, as the `i` pattern finds it.
fn has_p_blockquote(text: &[u8]) -> bool {
    memchr::memchr_iter(b'<', text).any(|i| {
        matches!(text.get(i + 1), Some(b'p' | b'P'))
            && text.get(i + 2) == Some(&b'>')
            && text.get(i..i + 14).is_some_and(|r| r.eq_ignore_ascii_case(b"<p><blockquote"))
    })
}

/// A `<blockquote` in any case: `<p><blockquote` can only appear later if the input has one.
fn has_blockquote(text: &[u8]) -> bool {
    memchr::memchr_iter(b'<', text).any(|i| {
        matches!(text.get(i + 1), Some(b'b' | b'B')) && text.get(i..i + 11).is_some_and(|r| r.eq_ignore_ascii_case(b"<blockquote"))
    })
}

/// `wp_replace_in_html_tags( $text, array( "\n" => ' <!-- wpnl --> ' ) )`, without collecting the chunks.
fn newlines_in_tags(text: &[u8]) -> Cow<'_, [u8]> {
    let mut out: Option<Vec<u8>> = None;
    let mut copied = 0;
    let mut offset = 0;
    for chunk in speedyspec_wp_html_split::split(text) {
        let bytes = chunk.bytes();
        let start = offset;
        offset += bytes.len();
        if matches!(chunk, speedyspec_wp_html_split::HtmlChunk::Tag(_)) && memchr::memchr(b'\n', bytes).is_some() {
            let buffer = out.get_or_insert_with(|| Vec::with_capacity(text.len() + 64));
            buffer.extend_from_slice(&text[copied..start]);
            buffer.extend_from_slice(&replace_one(bytes, b"\n", b" <!-- wpnl --> "));
            copied = offset;
        }
    }
    match out {
        None => Cow::Borrowed(text),
        Some(mut buffer) => {
            buffer.extend_from_slice(&text[copied..]);
            Cow::Owned(buffer)
        }
    }
}


/// `<br\s*/?>` at `p`; returns where it ends.
fn br_at(t: &[u8], p: usize) -> Option<usize> {
    if !t.get(p..)?.starts_with(b"<br") {
        return None;
    }
    let mut j = skip_spaces(t, p + 3);
    if t.get(j) == Some(&b'/') {
        j += 1;
    }
    (t.get(j) == Some(&b'>')).then_some(j + 1)
}

/// `(?<!<br />)\s*\n` replaced with `<br />\n`.
fn newline_br(t: &[u8]) -> Cow<'_, [u8]> {
    let mut out: Option<Vec<u8>> = None;
    let mut copied = 0;
    let mut from = 0;
    while let Some(at) = memchr::memchr(b'\n', &t[from..]) {
        let n = from + at;
        let mut s = n;
        while s > from && crate::scan::is_pcre_space(t[s - 1]) {
            s -= 1;
        }
        let r = skip_spaces(t, n);
        let last_nl = n + memchr::memrchr(b'\n', &t[n..r]).expect("n is a newline");
        let start = if t[..s].ends_with(b"<br />") { s + 1 } else { s };
        if start > last_nl {
            from = r;
            continue;
        }
        let buffer = out.get_or_insert_with(|| Vec::with_capacity(t.len() + t.len() / 4));
        buffer.extend_from_slice(&t[copied..start]);
        buffer.extend_from_slice(b"<br />\n");
        copied = last_nl + 1;
        from = last_nl + 1;
    }
    match out {
        None => Cow::Borrowed(t),
        Some(mut buffer) => {
            buffer.extend_from_slice(&t[copied..]);
            Cow::Owned(buffer)
        }
    }
}

/// `preg_split( '/\n\s*\n/', $text, -1, PREG_SPLIT_NO_EMPTY )`, each piece wrapped as `<p>…</p>\n`.
fn paragraphs(t: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(t.len() + t.len() / 8);
    let wrap = |out: &mut Vec<u8>, piece: &[u8]| {
        if !piece.is_empty() {
            out.extend_from_slice(b"<p>");
            out.extend_from_slice(php_trim(piece, b"\n"));
            out.extend_from_slice(b"</p>\n");
        }
    };
    let mut last = 0;
    let mut from = 0;
    while let Some(at) = memchr::memchr(b'\n', &t[from..]) {
        let n = from + at;
        let r = skip_spaces(t, n + 1);
        match memchr::memrchr(b'\n', &t[n + 1..r]) {
            Some(back) => {
                let end = n + 1 + back + 1;
                wrap(&mut out, &t[last..n]);
                last = end;
                from = end;
            }
            None => from = n + 1,
        }
    }
    wrap(&mut out, &t[last..]);
    out
}

/// WordPress's `wpautop()`, giving [`wpautop_reference`](crate::wpautop_reference)'s bytes faster.
///
/// @param text
///     The text.
/// @param options
///     Whether single line breaks become `<br />`.
/// @return
///     The text with paragraphs; whitespace-only input gives `""`.
pub fn wpautop(text: &[u8], options: AutopOptions) -> Cow<'_, [u8]> {
    if php_trim(text, PHP_TRIM_DEFAULT).is_empty() {
        return Cow::Borrowed(&text[..0]);
    }
    let p = &*PATTERNS;
    let early = presence(&EARLY, text);
    let blockquote = has_blockquote(text);
    let mut text = [text, b"\n"].concat();
    let mut pre_tags: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();

    if early & PRE != 0 {
        let mut rebuilt = Vec::with_capacity(text.len());
        let mut rest = &text[..];
        let mut i = 0;
        while let Some(at) = memmem::find(rest, b"</pre>") {
            let part = &rest[..at];
            match memmem::find(part, b"<pre") {
                None => rebuilt.extend_from_slice(part),
                Some(start) => {
                    let name = format!("<pre wp-pre-tag-{i}></pre>").into_bytes();
                    pre_tags.push((name.clone(), [&part[start..], b"</pre>"].concat()));
                    rebuilt.extend_from_slice(&part[..start]);
                    rebuilt.extend_from_slice(&name);
                    i += 1;
                }
            }
            rest = &rest[at + 6..];
        }
        rebuilt.extend_from_slice(rest);
        text = rebuilt;
    }

    // |<br\s*/?>\s*<br\s*/?>| → "\n\n"
    if early & BR != 0 {
        text = step(text, |t| {
            pass_needle(t, b"<br", |t, i, out| {
                let first = br_at(t, i)?;
                let second = br_at(t, skip_spaces(t, first))?;
                out.extend_from_slice(b"\n\n");
                Some(second)
            })
        });
    }
    // !(<BLOCK[\s/>])! → "\n\n$1", then !(</BLOCK>)! → "$1\n\n", in one pass: the first never matches at a `</`
    // and neither adds nor removes the other's text.
    text = step(text, |t| {
        pass(t, b'<', |t, i, out| {
            if t.get(i + 1) == Some(&b'/') {
                let end = exact_block(t, i + 2)?;
                if t.get(end) != Some(&b'>') {
                    return None;
                }
                out.extend_from_slice(&t[i..=end]);
                out.extend_from_slice(b"\n\n");
                return Some(end + 1);
            }
            let end = exact_block(t, i + 1)?;
            if !matches!(t.get(end), Some(b' ' | b'\t' | b'\n' | 0x0B | 0x0C | b'\r' | b'/' | b'>')) {
                return None;
            }
            out.extend_from_slice(b"\n\n");
            out.extend_from_slice(&t[i..=end]);
            Some(end + 1)
        })
    });
    // !(<hr\s*?/?>)! → "$1\n\n"
    if early & HR != 0 {
        text = step(text, |t| {
            pass_needle(t, b"<hr", |t, i, out| {
                let mut j = skip_spaces(t, i + 3);
                if t.get(j) == Some(&b'/') {
                    j += 1;
                }
                if t.get(j) != Some(&b'>') {
                    return None;
                }
                out.extend_from_slice(&t[i..=j]);
                out.extend_from_slice(b"\n\n");
                Some(j + 1)
            })
        });
    }
    if early & CR != 0 {
        text = CRLF.apply(&text).into_owned();
    }
    text = step(text, newlines_in_tags);

    let guards = presence(&GUARDS, &text);
    if guards & 1 != 0 {
        text = pattern(text, &p.option_before, b"<option");
        text = pattern(text, &p.option_after, b"</option>");
    }
    if guards & 0b10 != 0 {
        text = pattern(text, &p.object_open, b"$1");
        text = pattern(text, &p.object_close, b"</object>");
        text = pattern(text, &p.param_embed, b"$1");
    }
    if guards & 0b1100 != 0 {
        text = pattern(text, &p.media_open, b"$1");
        text = pattern(text, &p.media_close, b"$1");
        text = pattern(text, &p.source_track, b"$1");
    }
    if guards & 0b10000 != 0 {
        text = pattern(text, &p.figcaption_open, b"$1");
        text = pattern(text, &p.figcaption_close, b"</figcaption>");
    }

    // /\n\n+/ → "\n\n"
    text = step(text, |t| {
        pass_needle(t, b"\n\n\n", |t, i, out| {
            let end = i + t[i..].iter().take_while(|&&b| b == b'\n').count();
            out.extend_from_slice(b"\n\n");
            Some(end)
        })
    });
    text = paragraphs(&text);

    // |<p>\s*</p>| → ""
    text = step(text, |t| {
        pass_needle(t, b"<p>", |t, i, _| {
            let j = skip_spaces(t, i + 3);
            t[j..].starts_with(b"</p>").then_some(j + 4)
        })
    });
    // !<p>([^<]+)</(div|address|form)>! → "<p>$1</p></$2>"
    if early & UNCLOSED != 0 {
        text = step(text, |t| {
            pass_needle(t, b"<p>", |t, i, out| {
                let k = i + 3 + t[i + 3..].iter().take_while(|&&b| b != b'<').count();
                if k == i + 3 {
                    return None;
                }
                const NAMES: [&[u8]; 3] = [b"div", b"address", b"form"];
                let name = NAMES.into_iter().find(|n| {
                    t[k..].starts_with(b"</") && t[k + 2..].starts_with(n) && t.get(k + 2 + n.len()) == Some(&b'>')
                })?;
                out.extend_from_slice(&t[i..k]);
                out.extend_from_slice(b"</p></");
                out.extend_from_slice(name);
                out.push(b'>');
                Some(k + 3 + name.len())
            })
        });
    }
    // !<p>\s*(</?BLOCK[^>]*>)\s*</p>! → "$1"
    text = step(text, |t| {
        pass_needle(t, b"<p>", |t, i, out| {
            let tag = skip_spaces(t, i + 3);
            let end = block_tag(t, tag, starts_with_block)?;
            let close = skip_spaces(t, end);
            if !t[close..].starts_with(b"</p>") {
                return None;
            }
            out.extend_from_slice(&t[tag..end]);
            Some(close + 4)
        })
    });
    // |<p>(<li.+?)</p>| → "$1"
    if early & LI != 0 {
        text = step(text, |t| {
            pass_needle(t, b"<p><li", |t, i, out| {
                let from = i + 7;
                if from > t.len() {
                    return None;
                }
                let close = from + memmem::find(&t[from..], b"</p>")?;
                if memchr::memchr(b'\n', &t[i + 6..close]).is_some() {
                    return None;
                }
                out.extend_from_slice(&t[i + 3..close]);
                Some(close + 4)
            })
        });
    }
    if blockquote && has_p_blockquote(&text) {
        text = pattern(text, &p.p_blockquote, b"<blockquote$1><p>");
    }
    text = step(text, |t| replace_one(t, b"</blockquote></p>", b"</p></blockquote>"));
    // !<p>\s*(</?BLOCK[^>]*>)! → "$1"
    text = step(text, |t| {
        pass_needle(t, b"<p>", |t, i, out| {
            let tag = skip_spaces(t, i + 3);
            let end = block_tag(t, tag, starts_with_block)?;
            out.extend_from_slice(&t[tag..end]);
            Some(end)
        })
    });
    // !(</?BLOCK[^>]*>)\s*</p>! → "$1"
    text = step(text, |t| {
        pass(t, b'<', |t, i, out| {
            let end = block_tag(t, i, starts_with_block)?;
            let close = skip_spaces(t, end);
            if !t[close..].starts_with(b"</p>") {
                return None;
            }
            out.extend_from_slice(&t[i..end]);
            Some(close + 4)
        })
    });

    if options.br {
        if early & PRESERVE != 0 {
            text = match preg_replace_callback(&p.preserve, &text, |m| {
                replace_one(m.full(), b"\n", b"<WPPreserveNewline />").into_owned()
            }) {
                Ok(Cow::Owned(out)) => out,
                Ok(Cow::Borrowed(_)) => text,
                Err(_) => Vec::new(),
            };
        }
        // str_replace( array( '<br>', '<br/>' ), '<br />' ): neither can overlap the other.
        if early & BR != 0 {
            text = step(text, |t| {
                pass_needle(t, b"<br", |t, i, out| {
                    let len = if t[i + 3..].starts_with(b">") {
                        4
                    } else if t[i + 3..].starts_with(b"/>") {
                        5
                    } else {
                        return None;
                    };
                    out.extend_from_slice(b"<br />");
                    Some(i + len)
                })
            });
        }
        text = step(text, newline_br);
        text = step(text, |t| replace_one(t, b"<WPPreserveNewline />", b"\n"));
    }

    if contains(&text, b"<br />") {
        // !(</?BLOCK[^>]*>)\s*<br />! → "$1"
        text = step(text, |t| {
            pass(t, b'<', |t, i, out| {
                let end = block_tag(t, i, starts_with_block)?;
                let br = skip_spaces(t, end);
                if !t[br..].starts_with(b"<br />") {
                    return None;
                }
                out.extend_from_slice(&t[i..end]);
                Some(br + 6)
            })
        });
        // !<br />(\s*</?(?:p|li|div|dl|dd|dt|th|pre|td|ul|ol)[^>]*>)! → "$1"
        text = step(text, |t| {
            pass_needle(t, b"<br />", |t, i, out| {
                let tag = skip_spaces(t, i + 6);
                let end = block_tag(t, tag, starts_with_br_block)?;
                out.extend_from_slice(&t[i + 6..end]);
                Some(end)
            })
        });
    }
    // |\n</p>$| → "</p>"
    if text.ends_with(b"\n</p>") {
        let at = text.len() - 5;
        text.splice(at..at + 1, []);
    } else if text.ends_with(b"\n</p>\n") {
        let at = text.len() - 6;
        text.splice(at..at + 1, []);
    }

    if !pre_tags.is_empty() {
        text = StrReplace::new(pre_tags).apply(&text).into_owned();
    }
    if contains(&text, b"<!-- wpnl -->") {
        text = WPNL.apply(&text).into_owned();
    }
    Cow::Owned(text)
}
