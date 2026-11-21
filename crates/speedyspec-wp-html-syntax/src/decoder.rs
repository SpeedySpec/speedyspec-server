//! `WP_HTML_Decoder`.

use std::borrow::Cow;

use crate::named_reference;

/// Where text is decoded: attribute values treat a named reference without `;` more strictly.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DecodeContext {
    Attribute,
    Data,
}

const REPLACEMENT: &[u8] = "\u{FFFD}".as_bytes();

/// HTML's remapping of numeric references in the C1 range to Windows-1252.
const WINDOWS_1252: [u32; 32] = [
    0x20AC, 0x81, 0x201A, 0x0192, 0x201E, 0x2026, 0x2020, 0x2021, 0x02C6, 0x2030, 0x0160, 0x2039,
    0x0152, 0x8D, 0x017D, 0x8F, 0x90, 0x2018, 0x2019, 0x201C, 0x201D, 0x2022, 0x2013, 0x2014,
    0x02DC, 0x2122, 0x0161, 0x203A, 0x0153, 0x9D, 0x017E, 0x0178,
];

/// `code_point_to_utf8_bytes()`: the UTF-8 for a code point, or U+FFFD where `mb_chr()` refuses it.
pub fn code_point_to_utf8(code_point: u32) -> Vec<u8> {
    match char::from_u32(code_point) {
        Some(c) => c.to_string().into_bytes(),
        None => REPLACEMENT.to_vec(),
    }
}

/// `read_character_reference()`: the decoded bytes of the reference at `at`, and how many bytes it spans.
pub fn read_character_reference(
    ctx: DecodeContext,
    text: &[u8],
    at: usize,
) -> Option<(Vec<u8>, usize)> {
    let length = text.len();
    if at + 1 >= length || text[at] != b'&' {
        return None;
    }
    if text[at + 1] == b'#' {
        if at + 2 >= length {
            return None;
        }
        let mut digits_at = at + 2;
        let (hex, max_digits) = if matches!(text[digits_at], b'x' | b'X') {
            digits_at += 1;
            (true, 6)
        } else {
            (false, 7)
        };
        let is_digit = |b: &u8| {
            if hex {
                b.is_ascii_hexdigit()
            } else {
                b.is_ascii_digit()
            }
        };
        let tail = text.get(digits_at..).unwrap_or_default();
        let zero_count = tail.iter().take_while(|b| **b == b'0').count();
        let digit_count = tail[zero_count..]
            .iter()
            .take_while(|b| is_digit(b))
            .count();
        let after_digits = digits_at + zero_count + digit_count;
        let end_of_span = if text.get(after_digits) == Some(&b';') {
            after_digits + 1
        } else {
            after_digits
        };
        if digit_count == 0 && zero_count == 0 {
            return None;
        }
        if digit_count == 0 || digit_count > max_digits {
            return Some((REPLACEMENT.to_vec(), end_of_span - at));
        }
        let digits = &tail[zero_count..zero_count + digit_count];
        let mut code_point: u32 = 0;
        for d in digits {
            let v = (*d as char)
                .to_digit(if hex { 16 } else { 10 })
                .unwrap_or(0);
            code_point = code_point * if hex { 16 } else { 10 } + v;
        }
        if (0x80..=0x9F).contains(&code_point) {
            code_point = WINDOWS_1252[(code_point - 0x80) as usize];
        }
        return Some((code_point_to_utf8(code_point), end_of_span - at));
    }
    let name_at = at + 1;
    if name_at + 2 > length {
        return None;
    }
    let (replacement, name_length) = named_reference(text, name_at)?;
    let after_name = name_at + name_length;
    if ctx != DecodeContext::Attribute || text[after_name - 1] == b';' || after_name >= length {
        return Some((replacement.to_vec(), after_name - at));
    }
    let follower = text[after_name];
    if follower == b'=' || follower.is_ascii_alphanumeric() {
        return None;
    }
    Some((replacement.to_vec(), after_name - at))
}

/// `decode()`: every character reference replaced; borrows when there is none.
pub fn decode(ctx: DecodeContext, text: &[u8]) -> Cow<'_, [u8]> {
    let mut decoded: Option<Vec<u8>> = None;
    let mut at = 0;
    let mut was_at = 0;
    while at < text.len() {
        let Some(found) = memchr::memchr(b'&', &text[at..]) else {
            break;
        };
        let reference_at = at + found;
        if let Some((replacement, token_length)) = read_character_reference(ctx, text, reference_at)
        {
            let out = decoded.get_or_insert_with(|| Vec::with_capacity(text.len()));
            out.extend_from_slice(&text[was_at..reference_at]);
            out.extend_from_slice(&replacement);
            at = reference_at + token_length;
            was_at = at;
            continue;
        }
        at = reference_at + 1;
    }
    match decoded {
        None => Cow::Borrowed(text),
        Some(mut out) => {
            out.extend_from_slice(&text[was_at..]);
            Cow::Owned(out)
        }
    }
}

/// `decode_attribute()`.
pub fn decode_attribute(text: &[u8]) -> Cow<'_, [u8]> {
    decode(DecodeContext::Attribute, text)
}

/// `decode_text_node()`.
pub fn decode_text_node(text: &[u8]) -> Cow<'_, [u8]> {
    decode(DecodeContext::Data, text)
}
