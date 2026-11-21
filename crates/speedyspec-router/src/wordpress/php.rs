//! PHP's string functions that WordPress's routing relies on, byte for byte.

/// PHP's `urlencode`: alphanumerics and `-_.` kept, space as `+`, every other byte as `%XX` in upper case.
pub fn php_urlencode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for b in value.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' => out.push(b as char),
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// PHP's `urldecode`: `+` as space, `%XX` decoded, a malformed escape kept as written. Invalid UTF-8 that results
/// is replaced; [`php_urldecode_bytes`] keeps the bytes, as rule matching needs.
pub fn php_urldecode(value: &str) -> String {
    String::from_utf8_lossy(&php_urldecode_bytes(value)).into_owned()
}

/// PHP's `urldecode`, as bytes.
pub fn php_urldecode_bytes(value: &str) -> Vec<u8> {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < bytes.len()
                && (bytes[i + 1] as char).is_ascii_hexdigit()
                && (bytes[i + 2] as char).is_ascii_hexdigit() =>
            {
                let hex = |b: u8| (b as char).to_digit(16).unwrap_or(0) as u8;
                out.push(hex(bytes[i + 1]) * 16 + hex(bytes[i + 2]));
                i += 2;
            }
            b => out.push(b),
        }
        i += 1;
    }
    out
}

/// PHP's `addslashes`: a backslash before `'`, `"` and `\`, and NUL as `\0`.
pub fn php_addslashes(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            '\'' | '"' | '\\' => {
                out.push('\\');
                out.push(c);
            }
            '\0' => out.push_str("\\0"),
            c => out.push(c),
        }
    }
    out
}
