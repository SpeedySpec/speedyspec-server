//! PHP's lenient `base64_decode()`, which WordPress uses on signatures and keys.

/// Decodes as PHP's `base64_decode()` does without `$strict`: every byte outside the base64 alphabet (padding
/// included, wherever it appears) is skipped, the remaining characters are read as one stream of 6-bit groups, and
/// bits left over that do not make a whole byte are dropped. A strict decoder would reject inputs WordPress accepts.
pub fn php_base64_decode(input: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(input.len() / 4 * 3 + 3);
    let mut buffer: u32 = 0;
    let mut bits = 0u32;
    for &byte in input {
        let Some(value) = sextet(byte) else { continue };
        buffer = (buffer << 6) | u32::from(value);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buffer >> bits) as u8);
            buffer &= (1 << bits) - 1;
        }
    }
    out
}

fn sextet(byte: u8) -> Option<u8> {
    match byte {
        b'A'..=b'Z' => Some(byte - b'A'),
        b'a'..=b'z' => Some(byte - b'a' + 26),
        b'0'..=b'9' => Some(byte - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}
