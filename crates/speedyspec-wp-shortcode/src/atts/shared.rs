//! The steps of `shortcode_parse_atts()` both parsers share.

use speedyspec_wp_shortcode_contracts::{AttKey, AttsParse, ParsedAtts};

/// One match of `get_shortcode_atts_regex()`, by the alternative that matched.
pub(crate) enum Found<'a> {
    /// Alternatives 1–3: `name=value`.
    Named(&'a [u8], &'a [u8]),
    /// Alternatives 7–8: a quoted positional value.
    Quoted(&'a [u8]),
    /// Alternative 9: a bare positional value.
    Bare(&'a [u8]),
}

/// PCRE's `\s` in non-UTF mode with the C locale.
pub(crate) fn is_space(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | 0x0B | 0x0C | b'\r')
}

/// `preg_replace( "/[\x{00a0}\x{200b}]+/u", ' ', $text )`: `None` where PHP returns `null` (invalid UTF-8).
pub(crate) fn normalize_spaces(text: &[u8]) -> Option<Vec<u8>> {
    let s = std::str::from_utf8(text).ok()?;
    let mut out = Vec::with_capacity(text.len());
    let mut in_run = false;
    for c in s.chars() {
        if c == '\u{00a0}' || c == '\u{200b}' {
            if !in_run {
                out.push(b' ');
            }
            in_run = true;
        } else {
            in_run = false;
            let mut buf = [0; 4];
            out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
        }
    }
    Some(out)
}

/// PHP's `stripcslashes()`.
pub(crate) fn stripcslashes(s: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        if s[i] != b'\\' || i + 1 >= s.len() {
            out.push(s[i]);
            i += 1;
            continue;
        }
        i += 1;
        let c = s[i];
        let simple = match c {
            b'n' => Some(b'\n'),
            b't' => Some(b'\t'),
            b'r' => Some(b'\r'),
            b'a' => Some(0x07),
            b'v' => Some(0x0B),
            b'b' => Some(0x08),
            b'f' => Some(0x0C),
            _ => None,
        };
        if let Some(b) = simple {
            out.push(b);
            i += 1;
            continue;
        }
        if c == b'x' && s.get(i + 1).is_some_and(u8::is_ascii_hexdigit) {
            let digits = s[i + 1..].iter().take(2).take_while(|b| b.is_ascii_hexdigit()).count();
            let text = std::str::from_utf8(&s[i + 1..i + 1 + digits]).expect("hex digits");
            out.push(u8::from_str_radix(text, 16).expect("hex"));
            i += 1 + digits;
            continue;
        }
        let digits = s[i..].iter().take(3).take_while(|b| (b'0'..=b'7').contains(b)).count();
        if digits > 0 {
            let text = std::str::from_utf8(&s[i..i + digits]).expect("octal digits");
            out.push(u32::from_str_radix(text, 8).expect("octal") as u8);
            i += digits;
        } else {
            out.push(c);
            i += 1;
        }
    }
    out
}

/// `1 !== preg_match( '/^[^<]*+(?:<[^>]*+>[^<]*+)*+$/', $value )` for a value containing `<`.
pub(crate) fn rejects_unclosed_html(value: &[u8]) -> bool {
    let n = value.len();
    let skip = |mut i: usize, stop: u8| {
        while i < n && value[i] != stop {
            i += 1;
        }
        i
    };
    let mut i = skip(0, b'<');
    loop {
        if i >= n || value[i] != b'<' {
            break;
        }
        let close = skip(i + 1, b'>');
        if close >= n {
            break;
        }
        i = skip(close + 1, b'<');
    }
    !(i == n || (i + 1 == n && value[i] == b'\n'))
}

/// Builds WordPress's array from the matches, then blanks values with unclosed HTML.
pub(crate) fn assemble<'a>(found: impl Iterator<Item = Found<'a>>) -> ParsedAtts {
    let mut atts = ParsedAtts::new();
    for f in found {
        match f {
            // `! empty( $m[1] )`: a name of "0" falls through, and later groups don't exist.
            Found::Named(name, value) if name != b"0" => {
                atts.insert(AttKey::from_php(name.to_ascii_lowercase()), stripcslashes(value));
            }
            Found::Named(..) => {}
            Found::Quoted(v) if !v.is_empty() => atts.push(stripcslashes(v)),
            Found::Quoted(_) => {}
            Found::Bare(v) => atts.push(stripcslashes(v)),
        }
    }
    for value in atts.values_mut() {
        if value.contains(&b'<') && rejects_unclosed_html(value) {
            value.clear();
        }
    }
    atts
}

/// Runs the whole function around a matcher over the normalized text.
pub(crate) fn parse_with(text: &[u8], matches: impl for<'a> Fn(&'a [u8]) -> Vec<Found<'a>>) -> AttsParse {
    match normalize_spaces(text) {
        Some(text) => AttsParse { atts: assemble(matches(&text).into_iter()), null_subject: false },
        None => AttsParse { atts: ParsedAtts::new(), null_subject: true },
    }
}
